// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Hover tests for Scala language support (Phase 2)
//
// These tests verify that hover tooltips work for Scala files, including:
// - Class declarations
// - Method signatures with types
// - Field declarations with types
// - Object declarations
// - Trait declarations
// - Generic types
// - Function types

// ========== Scala Class Hover Tests ==========

lsp_hover_test! {
    name: scala_simple_class,
    files: {
        "com/example/User.scala" => r#"
            package com.example

            class Us<|>er {
              val name: String = "John"
            }
        "#
    },
    hover: "com/example/User.scala", "<|>",
    contains: ["class User"]
}

lsp_hover_test! {
    name: scala_case_class,
    files: {
        "com/example/Person.scala" => r"
            package com.example

            case class Pers<|>on(name: String, age: Int)
        "
    },
    hover: "com/example/Person.scala", "<|>",
    contains: ["class Person"]
}

// ========== Scala Field Hover Tests ==========

lsp_hover_test! {
    name: scala_val_field_hover_shows_val,
    files: {
        "com/example/Labels.scala" => r#"
            package com.example

            class Labels {
              val ti<|>tle: String = "t"
            }
        "#
    },
    hover: "com/example/Labels.scala", "<|>",
    contains: ["val title"]
}

lsp_hover_test! {
    name: scala_var_field_hover_shows_var,
    files: {
        "com/example/Counter.scala" => r#"
            package com.example

            class Counter {
              var cou<|>nt: Int = 0
            }
        "#
    },
    hover: "com/example/Counter.scala", "<|>",
    contains: ["var count"]
}

// ========== Scala Object Hover Tests ==========

lsp_hover_test! {
    name: scala_object,
    files: {
        "com/example/Utilities.scala" => r#"
            package com.example

            object Utiliti<|>es {
              def doSomething(): Unit = {
                println("Hello")
              }
            }
        "#
    },
    hover: "com/example/Utilities.scala", "<|>",
    contains: ["object Utilities"]
}

// ========== Scala Trait Hover Tests ==========

lsp_hover_test! {
    name: scala_trait,
    files: {
        "com/example/Configurable.scala" => r"
            package com.example

            trait Configurab<|>le {
              def configure(): Unit
            }
        "
    },
    hover: "com/example/Configurable.scala", "<|>",
    contains: ["trait Configurable"]
}

// ========== Scala Method Hover Tests ==========

lsp_hover_test! {
    name: scala_simple_method,
    files: {
        "com/example/User.scala" => r#"
            package com.example

            class User {
              def get<|>Name(): String = "John"
            }
        "#
    },
    hover: "com/example/User.scala", "<|>",
    contains: ["def getName"]
}

lsp_hover_test! {
    name: scala_method_with_params,
    files: {
        "com/example/Calculator.scala" => r"
            package com.example

            object Calculator {
              def ad<|>d(x: Int, y: Int): Int = x + y
            }
        "
    },
    hover: "com/example/Calculator.scala", "<|>",
    contains: ["def add"]
}

lsp_hover_test! {
    name: scala_method_with_generic_return,
    files: {
        "com/example/Repository.scala" => r"
            package com.example

            class Repository {
              def find<|>All(): List[String] = List()
            }
        "
    },
    hover: "com/example/Repository.scala", "<|>",
    contains: ["def findAll"]
}

// ========== Scala Field Hover Tests ==========

lsp_hover_test! {
    name: scala_val_field,
    files: {
        "com/example/User.scala" => r#"
            package com.example

            class User {
              val nam<|>e: String = "John"
            }
        "#
    },
    hover: "com/example/User.scala", "<|>",
    contains: ["val name"]
}

lsp_hover_test! {
    name: scala_var_field,
    files: {
        "com/example/Counter.scala" => r"
            package com.example

            class Counter {
              var coun<|>t: Int = 0
            }
        "
    },
    hover: "com/example/Counter.scala", "<|>",
    contains: ["count"]
}

// ========== Scala Cross-File Hover Tests ==========

lsp_hover_test! {
    name: scala_cross_file_method,
    files: {
        "com/example/Client.scala" => r"
            package com.example

            object Client {
              def test(): Unit = {
                val user = new User()
                user.get<|>Name()
              }
            }
        ",
        "com/example/User.scala" => r#"
            package com.example

            class User {
              def getName(): String = "John"
            }
        "#
    },
    hover: "com/example/Client.scala", "<|>",
    contains: ["def getName"]
}

lsp_hover_test! {
    name: scala_hover_prefers_imported_owner_for_ambiguous_method,
    files: {
        "com/a/Status.scala" => r#"
            package com.a

            object Status {
              def work(): String = "a"
            }
        "#,
        "com/b/Status.scala" => r"
            package com.b

            object Status {
              def work(): Int = 42
            }
        ",
        "com/example/Main.scala" => r"
            package com.example

            import com.b.Status

            object Main {
              def run(): Unit = {
                Status.wo<|>rk()
              }
            }
        "
    },
    hover: "com/example/Main.scala", "<|>",
    contains: ["def work", "Int"]
}

lsp_hover_test! {
    name: scala_cross_file_class,
    files: {
        "com/example/Main.scala" => r"
            package com.example

            object Main {
              def main(args: Array[String]): Unit = {
                val user: Us<|>er = new User()
              }
            }
        ",
        "com/example/User.scala" => r#"
            package com.example

            class User {
              val name: String = "John"
            }
        "#
    },
    hover: "com/example/Main.scala", "<|>",
    contains: ["class User"]
}

// ========== Scala Java Interop Hover Tests ==========

lsp_hover_test! {
    name: java_hover_on_scala_method,
    files: {
        "com/example/Client.java" => r"
            package com.example;

            public class Client {
                public void test(Config config) {
                    config.addReconfig<|>urable(null);
                }
            }
        ",
        "com/example/Config.scala" => r"
            package com.example

            class Config {
              def addReconfigurable(r: Reconfigurable): Unit = {
                // implementation
              }
            }
        ",
        "com/example/Reconfigurable.scala" => r"
            package com.example

            trait Reconfigurable
        "
    },
    hover: "com/example/Client.java", "<|>",
    contains: ["addReconfigurable"]
}
