// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Type Reference Navigation Tests for Kotlin
//
// These tests verify that goto-definition works when clicking on type annotations.
// This is Phase 4 of Kotlin LSP development.

lsp_test_suite! {
    name: "Simple Type References",

    test navigate_from_string_type {
        files: {
            "com/example/Test.kt" => r#"
                package com.example

                fun main() {
                    val name: Str<|>ing = "John"
                }
            "#,
            "kotlin/String.kt" => r"
                package kotlin

                class String {
                    val length: Int = 0
                }
            "
        },
        nav: "com/example/Test.kt", "<|>" => "String.kt"
    }

    test navigate_from_int_type {
        files: {
            "com/example/Test.kt" => r"
                package com.example

                fun test(x: In<|>t) {
                    println(x)
                }
            ",
            "kotlin/Int.kt" => r"
                package kotlin

                class Int
            "
        },
        nav: "com/example/Test.kt", "<|>" => "Int.kt"
    }

    test navigate_from_return_type {
        files: {
            "com/example/Test.kt" => r#"
                package com.example

                fun getName(): Str<|>ing {
                    return "John"
                }
            "#,
            "kotlin/String.kt" => r"
                package kotlin

                class String
            "
        },
        nav: "com/example/Test.kt", "<|>" => "String.kt"
    }
}

lsp_test_suite! {
    name: "Custom Class Type References",

    test navigate_from_variable_type {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                fun main() {
                    val user: Us<|>er = User("John")
                }
            "#,
            "com/example/User.kt" => r"
                package com.example

                class User(val name: String)
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test navigate_from_parameter_type {
        files: {
            "com/example/Service.kt" => r"
                package com.example

                fun process(user: Us<|>er) {
                    println(user.name)
                }
            ",
            "com/example/User.kt" => r"
                package com.example

                class User(val name: String)
            "
        },
        nav: "com/example/Service.kt", "<|>" => "User.kt"
    }

    test navigate_from_qualified_type {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                import com.example.model.User

                fun main() {
                    val user: Us<|>er = User("John")
                }
            "#,
            "com/example/model/User.kt" => r"
                package com.example.model

                class User(val name: String)
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }
}

lsp_test_suite! {
    name: "Generic Type Arguments",

    test navigate_from_list_base_type {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val items: Li<|>st<String> = emptyList()
                }
            ",
            "kotlin/collections/List.kt" => r"
                package kotlin.collections

                interface List<T>
            "
        },
        nav: "com/example/Main.kt", "<|>" => "List.kt"
    }

    test navigate_from_list_with_user_type {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun test() {
                    val users: List<User> = emptyList()
                    val u: Us<|>er = users.first()
                }
            ",
            "com/example/User.kt" => r"
                package com.example

                class User(val name: String)
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test navigate_from_set_type {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun test() {
                    val items: Se<|>t<String> = emptySet()
                }
            ",
            "kotlin/collections/Set.kt" => r"
                package kotlin.collections

                interface Set<T>
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Set.kt"
    }

    test navigate_from_array_type {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun test() {
                    val arr: Arr<|>ay<Int> = emptyArray()
                }
            ",
            "kotlin/Array.kt" => r"
                package kotlin

                class Array<T>
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Array.kt"
    }
}

lsp_test_suite! {
    name: "Nullable Types",

    test navigate_from_nullable_string {
        files: {
            "com/example/Test.kt" => r"
                package com.example

                fun main() {
                    val name: Str<|>ing? = null
                }
            ",
            "kotlin/String.kt" => r"
                package kotlin

                class String
            "
        },
        nav: "com/example/Test.kt", "<|>" => "String.kt"
    }

    test navigate_from_nullable_custom_type {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val user: Us<|>er? = null
                }
            ",
            "com/example/User.kt" => r"
                package com.example

                class User(val name: String)
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test navigate_from_nullable_generic {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val users: Li<|>st<User>? = null
                }
            ",
            "kotlin/collections/List.kt" => r"
                package kotlin.collections

                interface List<T>
            "
        },
        nav: "com/example/Main.kt", "<|>" => "List.kt"
    }
}

lsp_test_suite! {
    name: "Nested Generics",

    test navigate_from_nested_generic_outer {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val data: Ma<|>p<String, List<User>> = emptyMap()
                }
            ",
            "kotlin/collections/Map.kt" => r"
                package kotlin.collections

                interface Map<K, V>
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Map.kt"
    }

    test navigate_from_nested_generic_inner_base {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val data: Map<String, Li<|>st<User>> = emptyMap()
                }
            ",
            "kotlin/collections/List.kt" => r"
                package kotlin.collections

                interface List<T>
            "
        },
        nav: "com/example/Main.kt", "<|>" => "List.kt"
    }

    test navigate_from_pair_type {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                fun test() {
                    val pair: Pa<|>ir<String, Int> = Pair("a", 1)
                }
            "#,
            "kotlin/Pair.kt" => r"
                package kotlin

                data class Pair<A, B>(val first: A, val second: B)
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Pair.kt"
    }
}

lsp_test_suite! {
    name: "Property Types",

    test navigate_from_property_type {
        files: {
            "com/example/User.kt" => r"
                package com.example

                class User(
                    val name: Str<|>ing,
                    val age: Int
                )
            ",
            "kotlin/String.kt" => r"
                package kotlin

                class String
            "
        },
        nav: "com/example/User.kt", "<|>" => "String.kt"
    }

    test navigate_from_var_property_type {
        files: {
            "com/example/Service.kt" => r#"
                package com.example

                class Service {
                    var user: Us<|>er = User("John")
                }
            "#,
            "com/example/User.kt" => r"
                package com.example

                class User(val name: String)
            "
        },
        nav: "com/example/Service.kt", "<|>" => "User.kt"
    }

    test navigate_from_lateinit_property_type {
        files: {
            "com/example/Service.kt" => r"
                package com.example

                class Service {
                    lateinit var user: Us<|>er
                }
            ",
            "com/example/User.kt" => r"
                package com.example

                class User(val name: String)
            "
        },
        nav: "com/example/Service.kt", "<|>" => "User.kt"
    }
}

lsp_test_suite! {
    name: "Same Package Types",

    test navigate_from_same_package_type {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                fun main() {
                    val user: Us<|>er = User("John")
                }
            "#,
            "com/example/User.kt" => r"
                package com.example

                class User(val name: String)
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test navigate_from_same_package_no_import {
        files: {
            "com/example/Service.kt" => r"
                package com.example

                class Service(val repo: Reposit<|>ory)
            ",
            "com/example/Repository.kt" => r"
                package com.example

                class Repository
            "
        },
        nav: "com/example/Service.kt", "<|>" => "Repository.kt"
    }
}
