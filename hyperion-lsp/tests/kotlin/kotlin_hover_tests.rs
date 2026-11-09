// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::lsp_test_framework::LspTest;

// Hover tests for Kotlin language support
//
// These tests verify that hover tooltips work for Kotlin files, including:
// - Class declarations (data, sealed, open, abstract)
// - Function declarations (suspend, inline, infix, operator)
// - Property declarations (val/var, nullable/non-nullable)
// - Object declarations (including companion objects)
// - Cross-file hover

#[test]
fn unresolved_receiver_does_not_hover_a_unique_workspace_method() {
    let mut test = LspTest::new();
    test.add_file(
        "unrelated/Worker.kt",
        "package unrelated\nclass Worker { fun run(): String = \"wrong\" }",
    );
    test.add_file(
        "app/Main.kt",
        "package app\nfun caller() { missing.ru<|>n() }",
    );

    let hover = test
        .hover("app/Main.kt", "<|>")
        .expect("hover request should not fail");
    assert!(hover.is_none(), "an unresolved receiver must fail closed");
}

// ========== Kotlin Class Hover Tests ==========

lsp_hover_test! {
    name: kotlin_simple_class,
    files: {
        "com/example/User.kt" => r#"
            package com.example

            class Us<|>er {
                val name: String = ""
            }
        "#
    },
    hover: "com/example/User.kt", "<|>",
    contains: ["class User"]
}

lsp_hover_test! {
    name: kotlin_data_class,
    files: {
        "com/example/User.kt" => r"
            package com.example

            data class Us<|>er(val name: String)
        "
    },
    hover: "com/example/User.kt", "<|>",
    contains: ["data class User"]
}

lsp_hover_test! {
    name: kotlin_sealed_class,
    files: {
        "com/example/Result.kt" => r"
            package com.example

            sealed class Res<|>ult
        "
    },
    hover: "com/example/Result.kt", "<|>",
    contains: ["sealed class Result"]
}

lsp_hover_test! {
    name: kotlin_abstract_class,
    files: {
        "com/example/Base.kt" => r"
            package com.example

            abstract class Ba<|>se
        "
    },
    hover: "com/example/Base.kt", "<|>",
    contains: ["abstract class Base"]
}

lsp_hover_test! {
    name: kotlin_open_class,
    files: {
        "com/example/Parent.kt" => r"
            package com.example

            open class Par<|>ent
        "
    },
    hover: "com/example/Parent.kt", "<|>",
    contains: ["open class Parent"]
}

// ========== Kotlin Function Hover Tests ==========

lsp_hover_test! {
    name: kotlin_simple_function,
    files: {
        "com/example/Utils.kt" => r#"
            package com.example

            fun gre<|>et(name: String) {
                println("Hello, $name")
            }
        "#
    },
    hover: "com/example/Utils.kt", "<|>",
    contains: ["fun greet"]
}

lsp_hover_test! {
    name: kotlin_function_with_return_type,
    files: {
        "com/example/Math.kt" => r"
            package com.example

            fun ad<|>d(a: Int, b: Int): Int {
                return a + b
            }
        "
    },
    hover: "com/example/Math.kt", "<|>",
    contains: ["fun add", "Int"]
}

lsp_hover_test! {
    name: kotlin_suspend_function,
    files: {
        "com/example/Network.kt" => r#"
            package com.example

            suspend fun fetchDa<|>ta(): String {
                return "data"
            }
        "#
    },
    hover: "com/example/Network.kt", "<|>",
    contains: ["suspend fun fetchData", "String"]
}

lsp_hover_test! {
    name: kotlin_inline_function,
    files: {
        "com/example/Inline.kt" => r"
            package com.example

            inline fun ru<|>n(block: () -> Unit) {
                block()
            }
        "
    },
    hover: "com/example/Inline.kt", "<|>",
    contains: ["inline fun run"]
}

lsp_hover_test! {
    name: kotlin_operator_function,
    files: {
        "com/example/Point.kt" => r"
            package com.example

            class Point(val x: Int, val y: Int) {
                operator fun plu<|>s(other: Point): Point {
                    return Point(x + other.x, y + other.y)
                }
            }
        "
    },
    hover: "com/example/Point.kt", "<|>",
    contains: ["operator fun plus"]
}

lsp_hover_test! {
    name: kotlin_infix_function,
    files: {
        "com/example/Math.kt" => r"
            package com.example

            class Int {
                infix fun ti<|>mes(other: Int): Int {
                    return this * other
                }
            }
        "
    },
    hover: "com/example/Math.kt", "<|>",
    contains: ["infix fun times"]
}

// ========== Kotlin Property Hover Tests ==========

lsp_hover_test! {
    name: kotlin_val_property,
    files: {
        "com/example/Config.kt" => r#"
            package com.example

            class Config {
                val na<|>me: String = "config"
            }
        "#
    },
    hover: "com/example/Config.kt", "<|>",
    contains: ["val name", "String"]
}

lsp_hover_test! {
    name: kotlin_var_property,
    files: {
        "com/example/Counter.kt" => r"
            package com.example

            class Counter {
                var cou<|>nt: Int = 0
            }
        "
    },
    hover: "com/example/Counter.kt", "<|>",
    contains: ["var count", "Int"]
}

lsp_hover_test! {
    name: kotlin_nullable_property,
    files: {
        "com/example/User.kt" => r"
            package com.example

            class User {
                val ema<|>il: String? = null
            }
        "
    },
    hover: "com/example/User.kt", "<|>",
    contains: ["val email", "String?"]
}

lsp_hover_test! {
    name: kotlin_non_nullable_property,
    files: {
        "com/example/User.kt" => r#"
            package com.example

            class User {
                val na<|>me: String = ""
            }
        "#
    },
    hover: "com/example/User.kt", "<|>",
    contains: ["val name", "String"]
}

// ========== Kotlin Object Hover Tests ==========

lsp_hover_test! {
    name: kotlin_object_declaration,
    files: {
        "com/example/Config.kt" => r#"
            package com.example

            object Con<|>fig {
                val version = "1.0"
            }
        "#
    },
    hover: "com/example/Config.kt", "<|>",
    contains: ["object Config"]
}

lsp_hover_test! {
    name: kotlin_companion_object_unnamed,
    files: {
        "com/example/User.kt" => r"
            package com.example

            class User {
                companion obj<|>ect {
                    fun create(): User = User()
                }
            }
        "
    },
    hover: "com/example/User.kt", "<|>",
    contains: ["companion object"]
}

lsp_hover_test! {
    name: kotlin_companion_object_named,
    files: {
        "com/example/User.kt" => r"
            package com.example

            class User {
                companion object Fact<|>ory {
                    fun create(): User = User()
                }
            }
        "
    },
    hover: "com/example/User.kt", "<|>",
    contains: ["companion object Factory"]
}

// ========== Kotlin Cross-File Hover Tests ==========

lsp_hover_test! {
    name: kotlin_hover_on_class_reference,
    files: {
        "com/example/Main.kt" => r"
            package com.example

            fun main() {
                val user: Us<|>er = User()
            }
        ",
        "com/example/User.kt" => r#"
            package com.example

            class User {
                val name: String = ""
            }
        "#
    },
    hover: "com/example/Main.kt", "<|>",
    contains: ["User"]
}

lsp_hover_test! {
    name: kotlin_hover_on_class_reference_prefers_imported_package,
    files: {
        "com/example/Main.kt" => r"
            package com.example

            import com.b.User

            fun main() {
                val user: Us<|>er = User()
            }
        ",
        "com/a/UserA.kt" => r#"
            package com.a

            class User {
                val name: String = "A"
            }
        "#,
        "com/b/UserB.kt" => r#"
            package com.b

            class User {
                val name: String = "B"
            }
        "#
    },
    hover: "com/example/Main.kt", "<|>",
    contains: ["User", "com/b/UserB.kt"]
}

lsp_hover_test! {
    name: kotlin_hover_on_function_call,
    files: {
        "com/example/Main.kt" => r#"
            package com.example

            fun main() {
                gre<|>et("World")
            }
        "#,
        "com/example/Greet.kt" => r#"
            package com.example

            fun greet(name: String) {
                println("Hello, $name")
            }
        "#
    },
    hover: "com/example/Main.kt", "<|>",
    contains: ["greet"]
}

lsp_hover_test! {
    name: kotlin_hover_on_data_class_reference,
    files: {
        "com/example/Main.kt" => r#"
            package com.example

            fun main() {
                val user: Us<|>er = User("John")
            }
        "#,
        "com/example/User.kt" => r"
            package com.example

            data class User(val name: String)
        "
    },
    hover: "com/example/Main.kt", "<|>",
    contains: ["User"]
}

// ========== Kotlin Interface Hover Tests ==========

lsp_hover_test! {
    name: kotlin_interface_declaration,
    files: {
        "com/example/Clickable.kt" => r"
            package com.example

            interface Click<|>able {
                fun click()
            }
        "
    },
    hover: "com/example/Clickable.kt", "<|>",
    contains: ["interface Clickable"]
}

// ========== Kotlin Mixed Modifiers Tests ==========

lsp_hover_test! {
    name: kotlin_suspend_inline_function,
    files: {
        "com/example/Async.kt" => r"
            package com.example

            suspend inline fun withCont<|>ext(block: () -> Unit) {
                block()
            }
        "
    },
    hover: "com/example/Async.kt", "<|>",
    contains: ["suspend", "inline", "fun withContext"]
}

lsp_hover_test! {
    name: kotlin_data_sealed_combination,
    files: {
        "com/example/Result.kt" => r"
            package com.example

            sealed class Result
            data class Succ<|>ess(val value: String) : Result()
        "
    },
    hover: "com/example/Result.kt", "<|>",
    contains: ["data class Success"]
}

// ========== Kotlin Lambda Parameter Hover Tests ==========

lsp_hover_test! {
    name: kotlin_lambda_it_parameter_filter,
    files: {
        "com/example/Main.kt" => r"
            package com.example

            fun main() {
                val list = listOf(1, 2, 3)
                list.filter { i<|>t > 0 }
            }
        "
    },
    hover: "com/example/Main.kt", "<|>",
    contains: ["it", "Lambda parameter"]
}

lsp_hover_test! {
    name: kotlin_lambda_explicit_param,
    files: {
        "com/example/Main.kt" => r"
            package com.example

            fun main() {
                val list = listOf(1, 2, 3)
                list.map { x<|> -> x * 2 }
            }
        "
    },
    hover: "com/example/Main.kt", "<|>",
    contains: ["x", "Lambda parameter"]
}

// ========== Kotlin Visibility and Override Modifier Tests ==========

lsp_hover_test! {
    name: kotlin_override_function,
    files: {
        "com/example/Child.kt" => r#"
            package com.example

            open class Parent {
                open fun greet(): String = "Hello"
            }

            class Child : Parent() {
                override fun gre<|>et(): String = "Hi"
            }
        "#
    },
    hover: "com/example/Child.kt", "<|>",
    contains: ["override", "fun", "greet"]
}

lsp_hover_test! {
    name: kotlin_private_function,
    files: {
        "com/example/Utils.kt" => r"
            package com.example

            class Utils {
                private fun hel<|>per(): Int = 42
            }
        "
    },
    hover: "com/example/Utils.kt", "<|>",
    contains: ["private", "fun", "helper"]
}

lsp_hover_test! {
    name: kotlin_internal_class,
    files: {
        "com/example/Internal.kt" => r"
            package com.example

            internal class Inter<|>nalData {
                val value: Int = 0
            }
        "
    },
    hover: "com/example/Internal.kt", "<|>",
    contains: ["internal", "class", "InternalData"]
}
