// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Goto-definition tests for Kotlin language support
//
// These tests verify that goto-definition works for Kotlin files, including:
// - Cross-file navigation (class, function, property references)
// - Same-file navigation (local variables, parameters, etc.)

lsp_test_suite! {
    name: "Kotlin Cross-File Navigation",

    test kotlin_class_reference {
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
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test kotlin_function_call {
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
        nav: "com/example/Main.kt", "<|>" => "Greet.kt"
    }

    test kotlin_property_access {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val user = User()
                    println(user.na<|>me)
                }
            ",
            "com/example/User.kt" => r#"
                package com.example

                class User {
                    val name: String = "John"
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test kotlin_method_call {
        files: {
            "com/example/Client.kt" => r"
                package com.example

                fun test() {
                    val user = User()
                    user.get<|>Name()
                }
            ",
            "com/example/User.kt" => r#"
                package com.example

                class User {
                    fun getName(): String {
                        return "John"
                    }
                }
            "#
        },
        nav: "com/example/Client.kt", "<|>" => "User.kt"
    }

    test kotlin_class_reference_prefers_imported_package {
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
        nav: "com/example/Main.kt", "<|>" => "UserB.kt"
    }
}

lsp_test_suite! {
    name: "Kotlin Same-File Navigation",

    test kotlin_local_variable {
        files: {
            "com/example/Test.kt" => r#"
                package com.example

                fun test() {
                    val message = "Hello"
                    println(mes<|>sage)
                }
            "#
        },
        nav: "com/example/Test.kt", "<|>" => "Test.kt"
    }

    test kotlin_function_parameter {
        files: {
            "com/example/Test.kt" => r#"
                package com.example

                fun greet(name: String) {
                    println("Hello, $na<|>me")
                }
            "#
        },
        nav: "com/example/Test.kt", "<|>" => "Test.kt"
    }

    test kotlin_class_to_definition {
        files: {
            "com/example/Test.kt" => r#"
                package com.example

                class Us<|>er {
                    val name: String = ""
                }
            "#
        },
        nav: "com/example/Test.kt", "<|>" => "Test.kt"
    }

    test kotlin_function_to_definition {
        files: {
            "com/example/Test.kt" => r#"
                package com.example

                fun te<|>st() {
                    println("test")
                }
            "#
        },
        nav: "com/example/Test.kt", "<|>" => "Test.kt"
    }
}

lsp_test_suite! {
    name: "Kotlin Declaration Types",

    test kotlin_object_declaration {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    Con<|>fig.value
                }
            ",
            "com/example/Config.kt" => r"
                package com.example

                object Config {
                    val value = 42
                }
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Config.kt"
    }

    test kotlin_data_class {
        files: {
            "com/example/Main.kt" => r#"
                package com.example

                fun main() {
                    val user: Us<|>er = User("John", 30)
                }
            "#,
            "com/example/User.kt" => r"
                package com.example

                data class User(
                    val name: String,
                    val age: Int
                )
            "
        },
        nav: "com/example/Main.kt", "<|>" => "User.kt"
    }

    test kotlin_companion_object_member {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val instance = Factory.cre<|>ate()
                }
            ",
            "com/example/Factory.kt" => r"
                package com.example

                class Factory {
                    companion object {
                        fun create(): Factory {
                            return Factory()
                        }
                    }
                }
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Factory.kt"
    }
}

lsp_test_suite! {
    name: "Kotlin Top-Level Functions",

    test kotlin_top_level_function {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    helper<|>Function()
                }
            ",
            "com/example/Utils.kt" => r#"
                package com.example

                fun helperFunction() {
                    println("Helper")
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "Utils.kt"
    }

    test kotlin_top_level_property {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    println(CON<|>STANT)
                }
            ",
            "com/example/Constants.kt" => r"
                package com.example

                const val CONSTANT = 42
            "
        },
        nav: "com/example/Main.kt", "<|>" => "Constants.kt"
    }
}
