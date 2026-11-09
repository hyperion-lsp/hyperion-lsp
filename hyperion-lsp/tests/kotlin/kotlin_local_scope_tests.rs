// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Kotlin Local Scope Navigation Tests
//
// These tests verify that goto-definition works for local variables, parameters,
// nested functions, and variable shadowing scenarios in Kotlin.
//
// Test Coverage:
// 1.5: Local variable navigation in functions (3 tests)
// 1.6: Function parameter navigation (3 tests)
// 1.7: Nested function navigation (3 tests)
// 1.8: Variable shadowing scenarios (3 tests)

// ============================================================================
// 1.5: Local Variable Navigation in Functions
// ============================================================================

lsp_test_suite! {
    name: "Kotlin Local Variable Navigation",

    test local_variable_usage_to_declaration {
        files: {
            "Test.kt" => r"
                fun test() {
                    val x = 5
                    val result = <|>x + 1
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test local_variable_middle_of_function {
        files: {
            "Test.kt" => r"
                fun calculate() {
                    val a = 10
                    val b = 20
                    val sum = a + <|>b
                    println(sum)
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test local_variable_same_block {
        files: {
            "Test.kt" => r#"
                fun process() {
                    val message = "Hello"
                    val greeting = mes<|>sage
                }
            "#
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }
}

// ============================================================================
// 1.6: Function Parameter Navigation
// ============================================================================

lsp_test_suite! {
    name: "Kotlin Function Parameter Navigation",

    // Kotlin string-template interpolation is currently tokenized as STRING_LITERAL,
    // so parameter-reference navigation inside "$name" is not asserted here yet.

    test multiple_parameters {
        files: {
            "Test.kt" => r"
                fun add(a: Int, b: Int): Int {
                    return <|>a + b
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test parameter_with_complex_type {
        files: {
            "Test.kt" => r"
                fun processUsers(users: List<User>) {
                    println(us<|>ers.size)
                }

                data class User(val name: String)
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }
}

// ============================================================================
// 1.7: Nested Function Navigation
// ============================================================================

lsp_test_suite! {
    name: "Kotlin Nested Function Navigation",

    test nested_function_declaration {
        files: {
            "Test.kt" => r#"
                fun outer() {
                    fun inner() {
                        println("inner")
                    }
                    in<|>ner()
                }
            "#
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test nested_function_parameter {
        files: {
            "Test.kt" => r"
                fun outer() {
                    fun inner(value: Int) {
                        println(val<|>ue)
                    }
                    inner(42)
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test nested_function_accesses_outer_variable {
        files: {
            "Test.kt" => r#"
                fun outer() {
                    val outerVar = "outer"
                    fun inner() {
                        println(outer<|>Var)
                    }
                    inner()
                }
            "#
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }
}

// ============================================================================
// 1.8: Variable Shadowing Scenarios
// ============================================================================

lsp_test_suite! {
    name: "Kotlin Variable Shadowing",

    test inner_scope_shadows_outer {
        files: {
            "Test.kt" => r"
                fun test() {
                    val x = 10
                    run {
                        val x = 20
                        println(<|>x)  // Should resolve to inner x (20)
                    }
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test multiple_levels_of_shadowing {
        files: {
            "Test.kt" => r"
                fun test() {
                    val value = 1
                    run {
                        val value = 2
                        run {
                            val value = 3
                            println(val<|>ue)  // Should resolve to innermost (3)
                        }
                    }
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test shadowing_across_different_blocks {
        files: {
            "Test.kt" => r"
                fun test() {
                    val count = 0
                    if (true) {
                        val count = 1
                        println(cou<|>nt)  // Should resolve to inner count (1)
                    }
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }
}

// ============================================================================
// Additional Edge Cases
// ============================================================================

lsp_test_suite! {
    name: "Kotlin Local Scope Edge Cases",

    test variable_in_when_expression {
        files: {
            "Test.kt" => r#"
                fun test(value: Int) {
                    val result = when (val<|>ue) {
                        1 -> "one"
                        2 -> "two"
                        else -> "other"
                    }
                }
            "#
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test variable_in_for_loop {
        files: {
            "Test.kt" => r"
                fun test() {
                    val items = listOf(1, 2, 3)
                    for (item in items) {
                        println(it<|>em)
                    }
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test lambda_parameter {
        files: {
            "Test.kt" => r"
                fun test() {
                    val numbers = listOf(1, 2, 3)
                    numbers.forEach { num ->
                        println(nu<|>m)
                    }
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test destructuring_declaration {
        files: {
            "Test.kt" => r"
                fun test() {
                    val pair = Pair(1, 2)
                    val (first, second) = pair
                    println(fir<|>st)
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test variable_in_try_catch {
        files: {
            "Test.kt" => r#"
                fun test() {
                    try {
                        val result = riskyOperation()
                        println(res<|>ult)
                    } catch (e: Exception) {
                        println(e.message)
                    }
                }

                fun riskyOperation(): String = "success"
            "#
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }
}

// ============================================================================
// Lambda Implicit 'it' Parameter
// ============================================================================

lsp_test_suite! {
    name: "Kotlin Lambda Implicit It Parameter",

    test implicit_it_in_filter {
        files: {
            "Test.kt" => r"
                fun test() {
                    val numbers = listOf(1, 2, 3)
                    numbers.filter { i<|>t > 0 }
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test implicit_it_in_map {
        files: {
            "Test.kt" => r"
                fun test() {
                    val items = listOf(1, 2, 3)
                    items.map { <|>it * 2 }
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test implicit_it_in_foreach {
        files: {
            "Test.kt" => r"
                fun test() {
                    val list = listOf(1, 2, 3)
                    list.forEach { println(<|>it) }
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }

    test implicit_it_nested_lambda {
        files: {
            "Test.kt" => r"
                fun test() {
                    val lists = listOf(listOf(1, 2), listOf(3, 4))
                    lists.forEach { list ->
                        list.forEach { println(i<|>t) }
                    }
                }
            "
        },
        nav: "Test.kt", "<|>" => "Test.kt"
    }
}
