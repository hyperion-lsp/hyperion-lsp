// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Production-ready integration tests for Kotlin LSP v0.2.0
//
// These tests cover multi-feature patterns and edge cases to validate
// production readiness before release.

lsp_test_suite! {
    name: "Cross-File Advanced Navigation",

    test builder_pattern_across_files {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                fun main() {
                    val user = User.builder()
                        .name("John")
                        .age(30)
                        .bui<|>ld()
                }
            "#,
            "com/example/User.kt" => r#"
                package com.example

                class User private constructor(
                    val name: String,
                    val age: Int
                ) {
                    companion object {
                        fun builder(): Builder = Builder()
                    }

                    class Builder {
                        private var name: String = ""
                        private var age: Int = 0

                        fun name(name: String): Builder {
                            this.name = name
                            return this
                        }

                        fun age(age: Int): Builder {
                            this.age = age
                            return this
                        }

                        fun build(): User {
                            return User(name, age)
                        }
                    }
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test generic_collection_method_call {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                fun main() {
                    val users = listOf(User("Alice"), User("Bob"))
                    val names = users.map { it.ge<|>tName() }
                }
            "#,
            "com/example/User.kt" => r"
                package com.example

                data class User(private val name: String) {
                    fun getName(): String = name
                }
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test generic_collection_method_call_with_typed_annotation {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                fun main() {
                    val users: List<User> = listOf(User("Alice"), User("Bob"))
                    val names = users.map { it.ge<|>tName() }
                }
            "#,
            "com/example/User.kt" => r"
                package com.example

                data class User(private val name: String) {
                    fun getName(): String = name
                }
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test generic_collection_method_call_with_generic_ctor {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                fun main() {
                    val users = listOf<User>(User("Alice"), User("Bob"))
                    val names = users.map { it.ge<|>tName() }
                }
            "#,
            "com/example/User.kt" => r"
                package com.example

                data class User(private val name: String) {
                    fun getName(): String = name
                }
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test nested_class_navigation {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val config = Container.Con<|>fig()
                }
            ",
            "com/example/Container.kt" => r"
                package com.example

                class Container {
                    class Config {
                        var timeout: Int = 30
                    }
                }
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Container.kt"
    }
}

lsp_test_suite! {
    name: "Complex Type Patterns",

    // Note: Lambda with receiver is a known limitation
    // (requires advanced type inference for lambda receivers)

    test companion_object_member_access {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val id = User.generate<|>Id()
                }
            ",
            "com/example/User.kt" => r#"
                package com.example

                class User {
                    companion object {
                        fun generateId(): String = "USER_001"
                    }
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test object_singleton_access {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val config = AppConfig.ge<|>tTimeout()
                }
            ",
            "com/example/AppConfig.kt" => r"
                package com.example

                object AppConfig {
                    fun getTimeout(): Int = 30
                }
            "
        },
        nav: "com/example/Main.kt", "<|>" => "AppConfig.kt"
    }
}

lsp_test_suite! {
    name: "Import Resolution Edge Cases",

    test wildcard_import_navigation {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.models.*

                fun main() {
                    val user = Us<|>er()
                }
            ",
            "com/example/models/User.kt" => r"
                package com.example.models

                class User
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test aliased_import_navigation {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                import com.example.models.User as UserModel

                fun main() {
                    val user = UserMo<|>del()
                }
            ",
            "com/example/models/User.kt" => r"
                package com.example.models

                class User
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }
}

lsp_test_suite! {
    name: "Method Chaining Patterns",

    test fluent_api_navigation {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                fun main() {
                    val query = Query()
                        .select("name")
                        .from("users")
                        .whe<|>re("age > 18")
                }
            "#,
            "com/example/Query.kt" => r"
                package com.example

                class Query {
                    fun select(field: String): Query = this
                    fun from(table: String): Query = this
                    fun where(condition: String): Query = this
                }
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Query.kt"
    }
}

// Note: Hover and completion tests are in separate dedicated test files:
// - kotlin_hover_type_tests.rs (27 tests, 96% pass rate)
// - kotlin_completion_tests.rs (17 tests, 100% pass rate)
