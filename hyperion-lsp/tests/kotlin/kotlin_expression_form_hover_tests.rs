// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// End-to-end hover coverage for the Kotlin expression forms that previously
// produced no type at all: lambdas (`it`), `when`, `try`/`catch` and string
// templates.
//
// NOTE: Kotlin hover draws on two type sources. `hyperion_kotlin`'s scope graph
// and `TypeExtractor` handle declarations, annotations and simple initializers;
// `hyperion_semantics::KotlinTypeInferenceEngine` handles real expression
// inference (LUB of `when`/`try`/`if` branches, lambda receivers, `this`). The
// engine wins outright for un-annotated `when`/`try`/`if` initializers, where
// the token-stream heuristic reports a branch condition instead of the branch
// result, and fills gaps everywhere else. These tests pin the user-visible
// behaviour so that improvements on either path cannot silently regress the
// other.

// ========== Lambda parameters ==========

lsp_hover_test! {
    name: kotlin_hover_it_in_map_over_int_list,
    files: {
        "Test.kt" => r"
            fun test() {
                val nums = listOf(1, 2)
                val doubled = nums.map { i<|>t * 2 }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["it", "Int"]
}

// Still weak: an *explicitly named* lambda parameter gets no type. The scope
// graph records no annotation for it, and the inference engine's
// `lambda_parameter_type` needs the receiver type of `nums`, which the Kotlin
// symbol table only carries for annotated declarations and constructor
// initializers - not for `listOf(1, 2)`. Implicit `it` works because the scope
// graph types it separately (see the test above).
lsp_hover_test! {
    name: kotlin_hover_explicit_lambda_parameter,
    files: {
        "Test.kt" => r"
            fun test() {
                val nums = listOf(1, 2)
                val doubled = nums.map { n<|>um -> num * 2 }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["num", "Lambda parameter"]
}

// The lambda's result type, which only the call-return path can supply.
lsp_hover_test! {
    name: kotlin_hover_map_call_result,
    files: {
        "Test.kt" => r"
            fun test() {
                val dou<|>bled = listOf(1, 2).map { it * 2 }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["doubled", "List<Int>"]
}

// ========== when-expressions ==========

lsp_hover_test! {
    name: kotlin_hover_when_result_variable,
    files: {
        "Test.kt" => r#"
            fun classify(x: Int): String {
                val la<|>bel: String = when (x) {
                    1 -> "one"
                    else -> "many"
                }
                return label
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["label", "String"]
}

lsp_hover_test! {
    name: kotlin_hover_when_subject_binding,
    files: {
        "Test.kt" => r#"
            fun classify(): String {
                when (val su<|>bject = "text") {
                    else -> return subject
                }
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["subject", "String"]
}

// Un-annotated `when`: the type is the least upper bound of the branch
// *results*. The token-stream heuristic used to answer `Int` here - the type of
// the `1 ->` branch condition.
lsp_hover_test! {
    name: kotlin_hover_when_result_variable_without_annotation,
    files: {
        "Test.kt" => r#"
            fun classify(x: Int) {
                val <|>r = when (x) {
                    1 -> "one"
                    else -> "many"
                }
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["r", "String"]
}

// The same LUB has to hold at *use* sites, which read the type the scope graph
// derived at the declaration.
lsp_hover_test! {
    name: kotlin_hover_when_result_variable_at_use_site,
    files: {
        "Test.kt" => r#"
            fun classify(x: Int) {
                val r = when (x) {
                    1 -> "one"
                    else -> "many"
                }
                println(<|>r)
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["r", "String"]
}

// ========== try-expressions ==========

lsp_hover_test! {
    name: kotlin_hover_try_result_variable,
    files: {
        "Test.kt" => r"
            fun risky(): Int {
                val val<|>ue: Int = try {
                    1
                } catch (e: Exception) {
                    2
                }
                return value
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["value", "Int"]
}

// Un-annotated `try`: `Int` and `Long` branches widen to `Long`.
lsp_hover_test! {
    name: kotlin_hover_try_result_variable_without_annotation,
    files: {
        "Test.kt" => r"
            fun risky() {
                val <|>t = try { 1 } catch (e: Exception) { 2L }
            }
        "
    },
    hover: "Test.kt", "<|>",
    contains: ["t", "Long"]
}

// ========== String templates ==========

lsp_hover_test! {
    name: kotlin_hover_string_template_variable,
    files: {
        "Test.kt" => r#"
            fun greet(name: String) {
                val mess<|>age = "hello $name"
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["message", "String"]
}

// ========== Scope-function receivers ==========

// `this` is not an identifier token at all, so hover used to return nothing
// inside a scope function. Its type is the `with` receiver.
lsp_hover_test! {
    name: kotlin_hover_this_inside_with_block,
    files: {
        "Test.kt" => r#"
            fun test() {
                val sb = StringBuilder()
                with(sb) {
                    th<|>is.append("x")
                }
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["this", "StringBuilder"]
}

lsp_hover_test! {
    name: kotlin_hover_with_receiver_variable,
    files: {
        "Test.kt" => r#"
            class Builder {
                fun append(text: String): Builder = this
            }

            fun build(b<|>uilder: Builder) {
                with(builder) {
                    append("x")
                }
            }
        "#
    },
    hover: "Test.kt", "<|>",
    contains: ["builder", "Builder"]
}
