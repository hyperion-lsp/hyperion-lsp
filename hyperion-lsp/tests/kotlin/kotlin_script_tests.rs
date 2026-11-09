// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Script (.kts) Support Tests
//!
//! Tests for LSP features in Kotlin script files.
//! Kotlin scripts can contain top-level code without wrapping in a class or function.

// Test 1: Simple variable in .kts file
lsp_goto_test! {
    name: kts_simple_variable,
    files: {
        "script.kts" => r#"
            val message = "Hello"
            println(mess<|>age)
        "#
    },
    nav: "script.kts", "<|>" => "script.kts"
}

// Test 2: Function definition in .kts file
lsp_goto_test! {
    name: kts_function_definition,
    files: {
        "script.kts" => r#"
            fun greet(name: String) = "Hello, $name!"

            val result = gre<|>et("World")
        "#
    },
    nav: "script.kts", "<|>" => "script.kts"
}

// Test 3: Class definition in .kts file
lsp_goto_test! {
    name: kts_class_definition,
    files: {
        "script.kts" => r#"
            class Person(val name: String)

            val person = Per<|>son("Alice")
        "#
    },
    nav: "script.kts", "<|>" => "script.kts"
}

// Test 4: build.gradle.kts style file
lsp_goto_test! {
    name: kts_gradle_build_style,
    files: {
        "build.gradle.kts" => r#"
            val javaVersion = "17"

            println("Java version: $java<|>Version")
        "#
    },
    nav: "build.gradle.kts", "<|>" => "build.gradle.kts"
}

// Test 5: settings.gradle.kts style file
lsp_goto_test! {
    name: kts_gradle_settings_style,
    files: {
        "settings.gradle.kts" => r#"
            val projectName = "myapp"

            println("Project: $project<|>Name")
        "#
    },
    nav: "settings.gradle.kts", "<|>" => "settings.gradle.kts"
}

// Test 6: Cross-file reference from .kts to .kt
lsp_goto_test! {
    name: kts_cross_file_to_kt,
    files: {
        "script.kts" => r"
            import com.example.Helper

            Hel<|>per.doSomething()
        ",
        "com/example/Helper.kt" => r"
            package com.example

            object Helper {
                fun doSomething() {}
            }
        "
    },
    nav: "script.kts", "<|>" => "Helper.kt"
}

// Test 7: Top-level function call in .kts file
lsp_goto_test! {
    name: kts_top_level_function_call,
    files: {
        "script.kts" => r"
            fun process(value: Int) = value * 2

            val result = proc<|>ess(42)
            println(result)
        "
    },
    nav: "script.kts", "<|>" => "script.kts"
}

// Test 8: Multiple variables in .kts
lsp_goto_test! {
    name: kts_multiple_variables,
    files: {
        "script.kts" => r#"
            val first = "Hello"
            val second = "World"
            val combined = "$first, $sec<|>ond!"
        "#
    },
    nav: "script.kts", "<|>" => "script.kts"
}
