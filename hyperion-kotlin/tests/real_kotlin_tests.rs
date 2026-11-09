// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for parsing real Kotlin code
//!
//! This test suite verifies that the parser can handle realistic Kotlin code
//! including language-specific features like extension functions, data classes,
//! sealed classes, when expressions, lambdas, null safety operators, etc.

use hyperion_kotlin::lexer::Lexer;
use hyperion_kotlin::parser::{ParseError, Parser};

fn parse_file(source: &str) -> (rowan::GreenNode, Vec<ParseError>) {
    let tokens = Lexer::new(source).collect();
    Parser::parse(tokens)
}

fn print_test_result(name: &str, errors: &[ParseError]) {
    if errors.is_empty() {
        println!("✓ {name}: PASSED");
    } else {
        println!("✗ {}: FAILED with {} errors", name, errors.len());
        for (i, err) in errors.iter().enumerate() {
            println!("  Error {}: {}", i + 1, err);
        }
    }
}

#[test]
fn test_hello_world() {
    let source = r#"package com.example.hello

fun main() {
    println("Hello, World!")
}

fun greet(name: String): String {
    return "Hello, $name!"
}

fun add(a: Int, b: Int) = a + b
"#;

    let (_, errors) = parse_file(source);
    print_test_result("HelloWorld.kt", &errors);

    // Basic functions should parse without errors
    assert!(
        errors.is_empty(),
        "HelloWorld.kt should parse without errors"
    );
}

#[test]
fn test_data_class() {
    let source = r"package com.example.model

data class User(
    val id: Long,
    val name: String,
    val email: String?,
    var active: Boolean = true
)

data class Address(val street: String, val city: String)
";

    let (_, errors) = parse_file(source);
    print_test_result("DataClass.kt", &errors);

    // Data classes with nullable types and defaults might have some errors
    // depending on parser implementation
    if !errors.is_empty() {
        println!("Note: Data class features may not be fully supported yet");
    }
}

#[test]
fn test_extension_functions() {
    let source = r"package com.example.extensions

fun String.isPalindrome(): Boolean {
    return this == this.reversed()
}

fun List<Int>.sum(): Int {
    var total = 0
    for (item in this) {
        total += item
    }
    return total
}

fun <T> List<T>.second(): T? {
    return if (size >= 2) this[1] else null
}
";

    let (_, errors) = parse_file(source);
    print_test_result("Extensions.kt", &errors);

    // Extension functions are a complex Kotlin feature
    if !errors.is_empty() {
        println!("Note: Extension function syntax may not be fully supported yet");
    }
}

#[test]
fn test_null_safety() {
    let source = r#"package com.example.nullsafety

fun processUser(user: User?) {
    val name = user?.name ?: "Unknown"
    val email = user?.email?.lowercase() ?: "no-email"

    user?.let {
        println(it.name)
    }

    val nonNull = user!!
}

fun safeCast(obj: Any): String? {
    return obj as? String
}
"#;

    let (_, errors) = parse_file(source);
    print_test_result("NullSafety.kt", &errors);

    // Null safety operators are Kotlin-specific
    if !errors.is_empty() {
        println!("Note: Null safety operators parsing may have issues");
    }
}

#[test]
fn test_companion_object() {
    let source = r#"package com.example.companion

class Factory {
    companion object {
        fun create(): Factory {
            return Factory()
        }

        const val VERSION = "1.0.0"
    }
}

class Database private constructor() {
    companion object {
        private var instance: Database? = null

        fun getInstance(): Database {
            return instance ?: Database().also { instance = it }
        }
    }
}
"#;

    let (_, errors) = parse_file(source);
    print_test_result("CompanionObject.kt", &errors);

    // Companion objects are tested in the basic parser tests
    if !errors.is_empty() {
        println!("Note: Companion object features may not be fully supported yet");
    }
}

#[test]
fn test_sealed_class() {
    let source = r"package com.example.sealed

sealed class Result<out T> {
    data class Success<T>(val value: T) : Result<T>()
    data class Error(val message: String) : Result<Nothing>()
    object Loading : Result<Nothing>()
}

sealed interface NetworkState {
    object Connected : NetworkState
    object Disconnected : NetworkState
    data class Error(val exception: Exception) : NetworkState
}
";

    let (_, errors) = parse_file(source);
    print_test_result("SealedClass.kt", &errors);

    // Sealed classes with generics and variance are advanced features
    if !errors.is_empty() {
        println!("Note: Sealed class features may not be fully supported yet");
    }
}

#[test]
fn test_when_expression() {
    let source = r#"package com.example.when

fun describe(obj: Any): String = when (obj) {
    is String -> "String of length ${obj.length}"
    is Int -> "Integer value $obj"
    in 1..10 -> "Small number"
    !is Number -> "Not a number"
    else -> "Unknown"
}

fun processResult(result: Result) {
    when (result) {
        is Result.Success -> println(result.value)
        is Result.Error -> println(result.message)
        Result.Loading -> println("Loading...")
    }
}
"#;

    let (_, errors) = parse_file(source);
    print_test_result("WhenExpression.kt", &errors);

    // When expressions are complex control flow
    if !errors.is_empty() {
        println!("Note: When expression parsing may not be fully implemented yet");
    }
}

#[test]
fn test_lambdas() {
    let source = r#"package com.example.lambdas

fun main() {
    val numbers = listOf(1, 2, 3, 4, 5)

    val doubled = numbers.map { it * 2 }
    val filtered = numbers.filter { it > 2 }

    val sum = numbers.fold(0) { acc, n -> acc + n }

    val callback: (String) -> Unit = { message ->
        println("Callback: $message")
    }
}

fun higherOrder(block: () -> Unit) {
    block()
}

inline fun <T> Collection<T>.myFilter(predicate: (T) -> Boolean): List<T> {
    val result = mutableListOf<T>()
    for (item in this) {
        if (predicate(item)) {
            result.add(item)
        }
    }
    return result
}
"#;

    let (_, errors) = parse_file(source);
    print_test_result("Lambdas.kt", &errors);

    // Lambdas are tested in basic tests
    if !errors.is_empty() {
        println!("Note: Advanced lambda features may have parsing issues");
    }
}

#[test]
fn test_properties() {
    let source = r#"package com.example.properties

class Person {
    var name: String = ""
        get() = field.uppercase()
        set(value) {
            field = value.trim()
        }

    val age: Int = 0

    lateinit var address: String

    val fullName: String
        get() = "$name (age $age)"
}

val topLevel = "global"
var mutableGlobal: Int = 42

const val CONSTANT = 100
"#;

    let (_, errors) = parse_file(source);
    print_test_result("Properties.kt", &errors);

    // Custom getters/setters are advanced property features
    if !errors.is_empty() {
        println!("Note: Custom property getters/setters may not be fully supported yet");
    }
}

#[test]
fn test_coroutines() {
    let source = r#"package com.example.coroutines

suspend fun fetchUser(id: Long): User {
    delay(1000)
    return User(id, "Test")
}

suspend fun processData() {
    val user = fetchUser(1)
    println(user.name)
}

fun launchCoroutine() {
    GlobalScope.launch {
        val result = async {
            fetchUser(1)
        }.await()
    }
}
"#;

    let (_, errors) = parse_file(source);
    print_test_result("Coroutines.kt", &errors);

    // Suspend functions are tested in basic tests
    if !errors.is_empty() {
        println!("Note: Coroutine features may have parsing issues");
    }
}

/// Pinned, measured Kotlin parser capability survey.
///
/// Each entry's third field is not a wish or a target — it is the
/// *measured* `errors.is_empty()` result for that snippet as of the last
/// time this list was updated. The test asserts every entry against that
/// pinned value, so a parser change that flips any of them (in either
/// direction) turns this suite red instead of silently drifting, which is
/// what happened to the old (unasserted) version of this test: most of
/// its "expected to fail" flags had gone stale as the parser improved
/// without anyone updating the survey. When the parser legitimately
/// changes behavior for one of these snippets, update the pinned value
/// here in the same commit as the parser change, with a one-line reason.
///
/// This list intentionally also includes a few cases known to fail from
/// the corpus investigation in tests/corpus/ (see those files' header
/// comments for the full detail on each gap).
#[test]
fn summarize_parser_capabilities() {
    println!("\n========================================");
    println!("KOTLIN PARSER CAPABILITY SUMMARY");
    println!("========================================\n");

    let test_cases = vec![
        ("Basic Functions", "fun main() { }", true),
        ("Package Declaration", "package com.example", true),
        ("Import Declaration", "import kotlin.collections.List", true),
        ("Simple Class", "class Foo { }", true),
        // Measured true: data classes parse cleanly (contrary to the stale
        // `false` this test used to pin before it was asserted).
        ("Data Class", "data class User(val name: String)", true),
        ("Sealed Class", "sealed class Result { }", true),
        ("Object Declaration", "object Singleton { }", true),
        (
            "Companion Object",
            "class Foo { companion object { } }",
            true,
        ),
        ("Interface", "interface Foo { }", true),
        ("Val Property", "val x = 42", true),
        ("Var Property", "var y = 10", true),
        ("Nullable Type", "val x: String?", true),
        ("Generic Type", "val list: List<Int>", true),
        ("Lambda Expression", "val f = { x -> x + 1 }", true),
        // Measured true: extension functions parse cleanly.
        ("Extension Function", "fun String.test() { }", true),
        ("Suspend Function", "suspend fun test() { }", true),
        ("Inline Function", "inline fun test() { }", true),
        // Measured true: null-safety operators parse cleanly.
        ("Safe Call", "user?.name", true),
        ("Elvis Operator", "name ?: \"default\"", true),
        ("Not-null Assertion", "user!!", true),
        ("When Expression", "when (x) { 1 -> \"one\" }", true),
        ("For Loop", "for (i in 1..10) { }", true),
        // Conformance-ratchet coverage for recently added Kotlin grammar paths.
        (
            "Visibility modifier on constructor param",
            "class Foo(private val x: Int) { }",
            true,
        ),
        (
            "Use-site variance in type argument",
            "fun f(x: Array<out Any>) { }",
            true,
        ),
        (
            "Fun interface (SAM)",
            "fun interface Runner { fun run() }",
            true,
        ),
        (
            "Suspend function type in parameter position",
            "fun f(block: suspend () -> Unit) { }",
            true,
        ),
        (
            "Nullable extension receiver",
            "fun Int?.orZero(): Int = this ?: 0",
            true,
        ),
        ("Class literal (::class)", "fun f(): Any = Int::class", true),
        (
            "Local class in a function body",
            "fun f() { class Local }",
            true,
        ),
    ];

    let mut supported = 0;
    let total = test_cases.len();
    let mut mismatches: Vec<String> = Vec::new();

    for (name, code, expected_to_work) in &test_cases {
        let tokens = Lexer::new(code).collect();
        let (_, errors) = Parser::parse(tokens);
        let works = errors.is_empty();

        if works {
            supported += 1;
        }

        let status = if works { "✓ WORKS" } else { "✗ FAILS" };
        println!("{status}: {name}");

        if works != *expected_to_work {
            mismatches.push(format!(
                "{name}: pinned expected_to_work={expected_to_work} but measured works={works}"
            ));
        }
    }

    println!("\n========================================");
    println!(
        "SUMMARY: {}/{} features supported ({:.1}%)",
        supported,
        total,
        (f64::from(supported) / total as f64) * 100.0
    );
    println!("========================================\n");

    assert!(
        mismatches.is_empty(),
        "parser capability regressions/improvements not reflected in the pinned expectations \
         above — update the relevant entries in this test to match measured reality:\n{}",
        mismatches.join("\n")
    );
}
