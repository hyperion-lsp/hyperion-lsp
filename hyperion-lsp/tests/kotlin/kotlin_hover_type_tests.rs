// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Phase 3: Type Information & Hover Tests
//
// These tests verify that hover displays type annotations for:
// - Local variables with type annotations
// - Function parameters
// - Properties
// - Generic types
// - Nullable types
// - Type references (hover on variable usage shows its type)

// ========== Hover on Local Variables ==========

lsp_hover_test! {
    name: kotlin_hover_local_variable_with_type,
    files: {
        "Test.kt" => r#"
            fun test() {
                val na<|>me: String = "John"
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["name", "String"]
}

lsp_hover_test! {
    name: kotlin_hover_local_variable_usage,
    files: {
        "Test.kt" => r#"
            fun test() {
                val name: String = "John"
                println(na<|>me)
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["name", "String"]
}

lsp_hover_test! {
    name: kotlin_hover_local_variable_int,
    files: {
        "Test.kt" => r"
            fun test() {
                val cou<|>nt: Int = 5
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["count", "Int"]
}

// ========== Hover on Function Parameters ==========

lsp_hover_test! {
    name: kotlin_hover_parameter_simple,
    files: {
        "Test.kt" => r"
            fun greet(na<|>me: String) {
                println(name)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["name", "String", "Parameter"]
}

lsp_hover_test! {
    name: kotlin_hover_parameter_usage,
    files: {
        "Test.kt" => r"
            fun greet(name: String) {
                println(na<|>me)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["name", "String"]
}

lsp_hover_test! {
    name: kotlin_hover_parameter_int,
    files: {
        "Test.kt" => r"
            fun add(a: Int, b<|>: Int): Int {
                return a + b
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["b", "Int", "Parameter"]
}

lsp_hover_test! {
    name: kotlin_hover_parameter_multiple,
    files: {
        "Test.kt" => r"
            fun process(id: Int, na<|>me: String, active: Boolean) {
                println(name)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["name", "String"]
}

// ========== Hover on Property Types ==========

lsp_hover_test! {
    name: kotlin_hover_property_simple,
    files: {
        "Test.kt" => r#"
            class User {
                val na<|>me: String = "John"
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["name", "String"]
}

lsp_hover_test! {
    name: kotlin_hover_property_var,
    files: {
        "Test.kt" => r"
            class Counter {
                var co<|>unt: Int = 0
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["count", "Int"]
}

// ========== Hover on Generic Types ==========

lsp_hover_test! {
    name: kotlin_hover_generic_list,
    files: {
        "Test.kt" => r#"
            fun test() {
                val na<|>mes: List<String> = listOf("a", "b")
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["names", "List<String>"]
}

lsp_hover_test! {
    name: kotlin_hover_generic_map,
    files: {
        "Test.kt" => r"
            fun test() {
                val ag<|>es: Map<String, Int> = mapOf()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["ages", "Map<String, Int>"]
}

lsp_hover_test! {
    name: kotlin_hover_nested_generic,
    files: {
        "Test.kt" => r"
            fun test() {
                val data<|>: Map<String, List<Int>> = mapOf()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["data", "Map<String, List<Int>>"]
}

lsp_hover_test! {
    name: kotlin_hover_generic_parameter,
    files: {
        "Test.kt" => r"
            fun process(ite<|>ms: List<String>) {
                println(items)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["items", "List<String>"]
}

// ========== Hover on Nullable Types ==========

lsp_hover_test! {
    name: kotlin_hover_nullable_string,
    files: {
        "Test.kt" => r"
            fun test() {
                val nam<|>e: String? = null
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["name", "String?"]
}

lsp_hover_test! {
    name: kotlin_hover_nullable_int,
    files: {
        "Test.kt" => r"
            fun test() {
                val co<|>unt: Int? = null
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["count", "Int?"]
}

lsp_hover_test! {
    name: kotlin_hover_nullable_generic,
    files: {
        "Test.kt" => r"
            fun test() {
                val ite<|>ms: List<String>? = null
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["items", "List<String>?"]
}

lsp_hover_test! {
    name: kotlin_hover_nullable_parameter,
    files: {
        "Test.kt" => r"
            fun greet(nam<|>e: String?) {
                println(name)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["name", "String?", "Parameter"]
}

// ========== Hover on Custom Types ==========

lsp_hover_test! {
    name: kotlin_hover_custom_type_variable,
    files: {
        "Test.kt" => r"
            class User

            fun test() {
                val us<|>er: User = User()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["user", "User"]
}

lsp_hover_test! {
    name: kotlin_hover_custom_type_parameter,
    files: {
        "Test.kt" => r"
            class User

            fun process(us<|>er: User) {
                println(user)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["user", "User"]
}

lsp_hover_test! {
    name: kotlin_hover_custom_generic,
    files: {
        "Test.kt" => r"
            class User

            fun test() {
                val use<|>rs: List<User> = listOf()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["users", "List<User>"]
}

// ========== Hover on Loop Variables ==========

lsp_hover_test! {
    name: kotlin_hover_loop_variable,
    files: {
        "Test.kt" => r"
            fun test() {
                val items = listOf(1, 2, 3)
                for (it<|>em in items) {
                    println(item)
                }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["item"]
}

// ========== Hover on Lambda Parameters ==========

lsp_hover_test! {
    name: kotlin_hover_lambda_parameter,
    files: {
        "Test.kt" => r"
            fun test() {
                val double = { x<|> -> x * 2 }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["x", "Lambda parameter"]
}

lsp_hover_test! {
    name: kotlin_hover_lambda_parameter_usage,
    files: {
        "Test.kt" => r"
            fun test() {
                val double = { x -> x<|> * 2 }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["x"]
}
