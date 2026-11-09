// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Phase 7: Basic Type Inference Tests
//
// These tests verify that type inference works for:
// - Literal types (5 -> Int, "hello" -> String, etc.)
// - Constructor calls (User() -> User, ArrayList<String>() -> ArrayList<String>)
// - Variables without explicit type annotations
//
// Success criteria:
// - Hover shows inferred types
// - Goto-definition works on inferred types

// ========== Literal Type Inference ==========

lsp_hover_test! {
    name: kotlin_infer_int_literal,
    files: {
        "Test.kt" => r"
            fun test() {
                val cou<|>nt = 5
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["count", "Int"]
}

lsp_hover_test! {
    name: kotlin_infer_long_literal,
    files: {
        "Test.kt" => r"
            fun test() {
                val bi<|>g = 9999999999L
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["big", "Long"]
}

lsp_hover_test! {
    name: kotlin_infer_float_literal,
    files: {
        "Test.kt" => r"
            fun test() {
                val p<|>i = 3.14f
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["pi", "Float"]
}

lsp_hover_test! {
    name: kotlin_infer_double_literal,
    files: {
        "Test.kt" => r"
            fun test() {
                val pri<|>ce = 19.99
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["price", "Double"]
}

lsp_hover_test! {
    name: kotlin_infer_string_literal,
    files: {
        "Test.kt" => r#"
            fun test() {
                val messa<|>ge = "hello"
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["message", "String"]
}

lsp_hover_test! {
    name: kotlin_infer_raw_string_literal,
    files: {
        "Test.kt" => r#"
            fun test() {
                val tex<|>t = """
                    multi
                    line
                """
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["text", "String"]
}

lsp_hover_test! {
    name: kotlin_infer_char_literal,
    files: {
        "Test.kt" => r"
            fun test() {
                val lette<|>r = 'a'
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["letter", "Char"]
}

lsp_hover_test! {
    name: kotlin_infer_boolean_true,
    files: {
        "Test.kt" => r"
            fun test() {
                val activ<|>e = true
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["active", "Boolean"]
}

lsp_hover_test! {
    name: kotlin_infer_boolean_false,
    files: {
        "Test.kt" => r"
            fun test() {
                val disabl<|>ed = false
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["disabled", "Boolean"]
}

lsp_hover_test! {
    name: kotlin_infer_null,
    files: {
        "Test.kt" => r"
            fun test() {
                val emp<|>ty = null
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["empty", "Nothing"]
}

// ========== Constructor Call Inference ==========

lsp_hover_test! {
    name: kotlin_infer_simple_constructor,
    files: {
        "User.kt" => r"
            class User(val name: String)
        ",
        "Test.kt" => r#"
            fun test() {
                val use<|>r = User("John")
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["user", "User"]
}

lsp_hover_test! {
    name: kotlin_infer_generic_constructor,
    files: {
        "Test.kt" => r"
            fun test() {
                val lis<|>t = ArrayList<String>()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["list", "ArrayList"]
}

lsp_hover_test! {
    name: kotlin_infer_map_constructor,
    files: {
        "Test.kt" => r"
            fun test() {
                val ma<|>p = HashMap<String, Int>()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["map", "HashMap"]
}

// ========== Explicit Types Still Work (No Regression) ==========

lsp_hover_test! {
    name: kotlin_explicit_type_still_works,
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

lsp_hover_test! {
    name: kotlin_explicit_type_takes_precedence,
    files: {
        "Test.kt" => r"
            fun test() {
                val num<|>: Long = 5
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["num", "Long"]
}

// ========== Type Inference on Variable Usage ==========

lsp_hover_test! {
    name: kotlin_infer_usage_int,
    files: {
        "Test.kt" => r"
            fun test() {
                val count = 5
                println(cou<|>nt)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["count", "Int"]
}

lsp_hover_test! {
    name: kotlin_infer_usage_string,
    files: {
        "Test.kt" => r#"
            fun test() {
                val message = "hello"
                println(messa<|>ge)
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["message", "String"]
}

lsp_hover_test! {
    name: kotlin_infer_usage_constructor,
    files: {
        "User.kt" => r"
            class User(val name: String)
        ",
        "Test.kt" => r#"
            fun test() {
                val user = User("John")
                println(use<|>r)
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["user", "User"]
}

// ========== Complex Scenarios ==========

lsp_hover_test! {
    name: kotlin_infer_multiple_variables,
    files: {
        "Test.kt" => r#"
            fun test() {
                val a = 1
                val b = "hello"
                val c = true
                println(a<|>)
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["a", "Int"]
}

lsp_hover_test! {
    name: kotlin_infer_in_nested_scope,
    files: {
        "Test.kt" => r#"
            fun test() {
                val outer = 5
                {
                    val inne<|>r = "hello"
                }
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["inner", "String"]
}

lsp_hover_test! {
    name: kotlin_infer_shadowed_variable,
    files: {
        "Test.kt" => r#"
            fun test() {
                val x = 5
                {
                    val x<|> = "hello"
                }
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["x", "String"]
}

// ========== Edge Cases ==========

lsp_hover_test! {
    name: kotlin_infer_negative_int,
    files: {
        "Test.kt" => r"
            fun test() {
                val negati<|>ve = -42
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["negative", "Int"]
}

lsp_hover_test! {
    name: kotlin_infer_hex_literal,
    files: {
        "Test.kt" => r"
            fun test() {
                val he<|>x = 0xFF
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["hex", "Int"]
}

lsp_hover_test! {
    name: kotlin_infer_binary_literal,
    files: {
        "Test.kt" => r"
            fun test() {
                val bi<|>n = 0b1010
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["bin", "Int"]
}

// ========== Phase 8B: Method Return Type Inference ==========
//
// These tests verify that method return types are inferred correctly.
// This enables hover to show types for variables assigned from method calls.

lsp_hover_test! {
    name: kotlin_method_return_string_length,
    files: {
        "Test.kt" => r#"
            fun test() {
                val text = "hello"
                val len<|> = text.length
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["len", "Int"]
}

lsp_hover_test! {
    name: kotlin_method_return_string_uppercase,
    files: {
        "Test.kt" => r#"
            fun test() {
                val text = "hello"
                val upper<|> = text.uppercase()
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["upper", "String"]
}

lsp_hover_test! {
    name: kotlin_method_return_list_size,
    files: {
        "Test.kt" => r#"
            fun test() {
                val items = listOf("a", "b", "c")
                val count<|> = items.size
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["count", "Int"]
}

lsp_hover_test! {
    name: kotlin_method_return_free_function,
    files: {
        "Test.kt" => r"
            fun test() {
                val list<|> = listOf(1, 2, 3)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["list", "List"]
}

lsp_hover_test! {
    name: kotlin_method_return_map_get,
    files: {
        "Test.kt" => r#"
            fun test() {
                val map = mapOf("key" to "value")
                val result<|> = map.get("key")
            }
        "#
    },
    hover: "Test.kt", "<|>",
    // Map.get() returns nullable type
    contains: ["result"]
}

lsp_hover_test! {
    name: kotlin_method_return_chained_calls,
    files: {
        "Test.kt" => r#"
            fun test() {
                val len<|> = "hello".uppercase().length
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["len", "Int"]
}

lsp_hover_test! {
    name: kotlin_method_return_number_conversion,
    files: {
        "Test.kt" => r"
            fun test() {
                val x = 42
                val str<|> = x.toString()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["str", "String"]
}

lsp_hover_test! {
    name: kotlin_method_return_boolean_method,
    files: {
        "Test.kt" => r#"
            fun test() {
                val text = "hello"
                val empty<|> = text.isEmpty()
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["empty", "Boolean"]
}

lsp_hover_test! {
    name: kotlin_method_return_empty_list,
    files: {
        "Test.kt" => r"
            fun test() {
                val list<|> = emptyList()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["list", "List"]
}

lsp_hover_test! {
    name: kotlin_method_return_println,
    files: {
        "Test.kt" => r#"
            fun test() {
                val result<|> = println("hello")
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["result", "Unit"]
}

// Test that explicit types still work alongside method inference
lsp_hover_test! {
    name: kotlin_method_return_with_explicit_type,
    files: {
        "Test.kt" => r#"
            fun test() {
                val text = "hello"
                val len<|>: Int = text.length
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["len", "Int"]
}

// Test recursive method return type resolution
lsp_hover_test! {
    name: kotlin_method_return_recursive,
    files: {
        "Test.kt" => r#"
            fun test() {
                val user = getUser()
                val name<|> = user.getName()
            }

            fun getUser(): User = User()
            class User {
                fun getName(): String = "John"
            }
        "#
    },
    hover: "Test.kt", "<|>",
    // Should show String if we resolve user.getName() correctly
    contains: ["name"]
}

// ========== Phase 8C: Generic Type Parameter Inference ==========
//
// These tests verify that generic type parameters are inferred from
// function arguments, particularly for collection constructors.

lsp_hover_test! {
    name: kotlin_generic_listof_int,
    files: {
        "Test.kt" => r"
            fun test() {
                val numbers<|> = listOf(1, 2, 3)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["numbers", "List", "Int"]
}

lsp_hover_test! {
    name: kotlin_generic_listof_string,
    files: {
        "Test.kt" => r#"
            fun test() {
                val names<|> = listOf("Alice", "Bob", "Charlie")
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["names", "List", "String"]
}

lsp_hover_test! {
    name: kotlin_generic_listof_boolean,
    files: {
        "Test.kt" => r"
            fun test() {
                val flags<|> = listOf(true, false, true)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["flags", "List", "Boolean"]
}

lsp_hover_test! {
    name: kotlin_generic_setof_int,
    files: {
        "Test.kt" => r"
            fun test() {
                val ids<|> = setOf(1, 2, 3)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["ids", "Set", "Int"]
}

lsp_hover_test! {
    name: kotlin_generic_setof_string,
    files: {
        "Test.kt" => r#"
            fun test() {
                val tags<|> = setOf("kotlin", "java", "scala")
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["tags", "Set", "String"]
}

lsp_hover_test! {
    name: kotlin_generic_arrayof_long,
    files: {
        "Test.kt" => r"
            fun test() {
                val bigNums<|> = arrayOf(1L, 2L, 3L)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["bigNums", "Array", "Long"]
}

lsp_hover_test! {
    name: kotlin_generic_arrayof_double,
    files: {
        "Test.kt" => r"
            fun test() {
                val prices<|> = arrayOf(1.99, 2.99, 3.99)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["prices", "Array", "Double"]
}

lsp_hover_test! {
    name: kotlin_generic_listof_empty,
    files: {
        "Test.kt" => r"
            fun test() {
                val empty<|> = listOf()
            }
        "
    },
    hover: "Test.kt", "<|>",
    // Empty list has Unknown type parameter
    contains: ["empty", "List"]
}

lsp_hover_test! {
    name: kotlin_generic_mutablelistof_int,
    files: {
        "Test.kt" => r"
            fun test() {
                val mutable<|> = mutableListOf(10, 20, 30)
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["mutable", "MutableList", "Int"]
}

lsp_hover_test! {
    name: kotlin_generic_hashsetof_string,
    files: {
        "Test.kt" => r#"
            fun test() {
                val unique<|> = hashSetOf("a", "b", "c")
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["unique", "HashSet", "String"]
}

// Test that explicit types still work alongside generic inference
lsp_hover_test! {
    name: kotlin_generic_explicit_type_override,
    files: {
        "Test.kt" => r"
            fun test() {
                val nums<|>: List<Long> = listOf(1, 2, 3)
            }
        "
    },
    hover: "Test.kt", "<|>",
    // Explicit type should take precedence
    contains: ["nums", "List", "Long"]
}

// ========== Builder Pattern Support ==========
//
// These tests verify that builder patterns work correctly with method chaining

lsp_hover_test! {
    name: kotlin_builder_pattern_basic,
    files: {
        "User.kt" => r"
            class User
            class UserBuilder {
                fun name(n: String): UserBuilder = this
                fun age(a: Int): UserBuilder = this
                fun build(): User = User()
            }
        ",
        "Test.kt" => r"
            fun test() {
                val builder<|> = UserBuilder()
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["builder", "UserBuilder"]
}

lsp_hover_test! {
    name: kotlin_builder_pattern_chain,
    files: {
        "User.kt" => r"
            class User
            class UserBuilder {
                fun name(n: String): UserBuilder = this
                fun age(a: Int): UserBuilder = this
                fun build(): User = User()
            }
        ",
        "Test.kt" => r#"
            fun test() {
                val builder<|> = UserBuilder().name("John")
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["builder", "UserBuilder"]
}

lsp_hover_test! {
    name: kotlin_builder_pattern_build,
    files: {
        "User.kt" => r"
            class User
            class UserBuilder {
                fun name(n: String): UserBuilder = this
                fun age(a: Int): UserBuilder = this
                fun build(): User = User()
            }
        ",
        "Test.kt" => r#"
            fun test() {
                val user<|> = UserBuilder().name("John").age(30).build()
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["user", "User"]
}

// ========== Lambda Higher-Order Functions ==========
//
// These tests verify that higher-order functions like map, filter, etc. work

lsp_hover_test! {
    name: kotlin_lambda_filter,
    files: {
        "Test.kt" => r"
            fun test() {
                val numbers = listOf(1, 2, 3, 4, 5)
                val evens<|> = numbers.filter { it % 2 == 0 }
            }
        "
    },
    hover: "Test.kt", "<|>",
    // filter returns same type
    contains: ["evens", "List", "Int"]
}

lsp_hover_test! {
    name: kotlin_lambda_foreach,
    files: {
        "Test.kt" => r"
            fun test() {
                val numbers = listOf(1, 2, 3)
                val result<|> = numbers.forEach { println(it) }
            }
        "
    },
    hover: "Test.kt", "<|>",
    // forEach returns Unit
    contains: ["result", "Unit"]
}

lsp_hover_test! {
    name: kotlin_lambda_any,
    files: {
        "Test.kt" => r"
            fun test() {
                val numbers = listOf(1, 2, 3)
                val hasEven<|> = numbers.any { it % 2 == 0 }
            }
        "
    },
    hover: "Test.kt", "<|>",
    // any returns Boolean
    contains: ["hasEven", "Boolean"]
}

lsp_hover_test! {
    name: kotlin_lambda_count,
    files: {
        "Test.kt" => r"
            fun test() {
                val numbers = listOf(1, 2, 3, 4, 5)
                val evenCount<|> = numbers.count { it % 2 == 0 }
            }
        "
    },
    hover: "Test.kt", "<|>",
    // count returns Int
    contains: ["evenCount", "Int"]
}

lsp_hover_test! {
    name: kotlin_lambda_sorted,
    files: {
        "Test.kt" => r"
            fun test() {
                val numbers = listOf(3, 1, 4, 1, 5)
                val sorted<|> = numbers.sorted()
            }
        "
    },
    hover: "Test.kt", "<|>",
    // sorted returns same List type
    contains: ["sorted", "List", "Int"]
}
