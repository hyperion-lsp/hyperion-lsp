// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Smart Cast Tests
//!
//! Tests for type narrowing after `is` checks.
//! When a variable is checked with `is`, the type should be narrowed
//! in the then-block for better hover information.

// Test 1: Simple is check - parameter to String
// After `if (x is String)`, x should be narrowed to String
lsp_hover_test! {
    name: kotlin_smart_cast_parameter_to_string,
    files: {
        "Test.kt" => r"
            package com.example

            fun test(x: Any) {
                if (x is String) {
                    <|>x.length
                }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["String"]
}

// Test 2: Smart cast should NOT apply outside the then-block
lsp_hover_test! {
    name: kotlin_smart_cast_not_outside_then,
    files: {
        "Test.kt" => r"
            package com.example

            fun test(x: Any) {
                if (x is String) {
                    // narrowed here
                }
                <|>x.hashCode()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["Any"]
}

// Test 3: Smart cast with custom class
lsp_hover_test! {
    name: kotlin_smart_cast_custom_class,
    files: {
        "Animal.kt" => r"
            package com.example

            open class Animal {
                fun eat() {}
            }

            class Dog : Animal() {
                fun bark() {}
            }
        ",
        "Test.kt" => r"
            package com.example

            fun test(animal: Animal) {
                if (animal is Dog) {
                    <|>animal.bark()
                }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["Dog"]
}

// Test 4: Smart cast in nested if blocks
lsp_hover_test! {
    name: kotlin_smart_cast_nested_if,
    files: {
        "Test.kt" => r"
            package com.example

            fun test(x: Any) {
                if (x is String) {
                    if (x.length > 0) {
                        <|>x.uppercase()
                    }
                }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["String"]
}

// Test 5: Smart cast on local variable (val)
lsp_hover_test! {
    name: kotlin_smart_cast_local_variable,
    files: {
        "Test.kt" => r#"
            package com.example

            fun test() {
                val obj: Any = "hello"
                if (obj is String) {
                    <|>obj.length
                }
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["String"]
}
