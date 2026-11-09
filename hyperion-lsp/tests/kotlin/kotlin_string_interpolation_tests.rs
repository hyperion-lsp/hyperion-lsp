// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin String Interpolation Tests
//!
//! Tests for goto-definition within string templates.
//! When clicking on $identifier in a string, should navigate to the variable definition.

// Test 1: Simple $identifier pattern
lsp_goto_test! {
    name: kotlin_string_template_simple,
    files: {
        "Test.kt" => r#"
            package com.example

            fun test() {
                val name = "World"
                println("Hello, $na<|>me!")
            }
        "#
    },
    nav: "Test.kt", "<|>" => "Test.kt"
}

// Test 2: $identifier at start of string
lsp_goto_test! {
    name: kotlin_string_template_at_start,
    files: {
        "Test.kt" => r#"
            package com.example

            fun test() {
                val prefix = "Hello"
                val message = "$pre<|>fix World"
            }
        "#
    },
    nav: "Test.kt", "<|>" => "Test.kt"
}

// Test 3: Multiple $identifiers in one string
lsp_goto_test! {
    name: kotlin_string_template_multiple,
    files: {
        "Test.kt" => r#"
            package com.example

            fun test() {
                val first = "Hello"
                val second = "World"
                val message = "$first, $sec<|>ond!"
            }
        "#
    },
    nav: "Test.kt", "<|>" => "Test.kt"
}

// Test 4: Function parameter reference
lsp_goto_test! {
    name: kotlin_string_template_param,
    files: {
        "Test.kt" => r#"
            package com.example

            fun greet(name: String) {
                println("Hello, $na<|>me!")
            }
        "#
    },
    nav: "Test.kt", "<|>" => "Test.kt"
}

// Test 5: ${expression} pattern - simple variable
lsp_goto_test! {
    name: kotlin_string_template_expr_simple,
    files: {
        "Test.kt" => r#"
            package com.example

            fun test() {
                val count = 42
                println("Count: ${cou<|>nt}")
            }
        "#
    },
    nav: "Test.kt", "<|>" => "Test.kt"
}

// Test 6: ${expression} pattern - property access (variable)
lsp_goto_test! {
    name: kotlin_string_template_expr_property,
    files: {
        "Test.kt" => r#"
            package com.example

            fun test() {
                val name = "World"
                println("Length: ${na<|>me.length}")
            }
        "#
    },
    nav: "Test.kt", "<|>" => "Test.kt"
}

// Note: Property navigation within ${expr} (like ${name.length} -> length)
// would require receiver type context, similar to regular property access.
// This is a potential future enhancement.
