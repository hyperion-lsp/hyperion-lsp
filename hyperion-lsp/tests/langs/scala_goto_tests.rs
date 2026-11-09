// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Goto-definition tests for Scala language support (Phase 0)
//
// These tests verify that Java code can navigate to Scala definitions.

lsp_test_suite! {
    name: "Java to Scala Navigation",

    test java_calls_scala_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(Config config) {
                        config.addReconfigurable<|>(null);
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
        nav: "com/example/Main.java", "<|>" => "Config.scala"
    }

    test java_references_scala_class {
        files: {
            "com/example/Client.java" => r"
                package com.example;

                public class Client {
                    public void test() {
                        Use<|>r user = new User();
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
        nav: "com/example/Client.java", "<|>" => "User.scala"
    }

    test java_references_scala_object {
        files: {
            "com/example/App.java" => r"
                package com.example;

                public class App {
                    public void test() {
                        Utilit<|>ies.doSomething();
                    }
                }
            ",
            "com/example/Utilities.scala" => r#"
                package com.example

                object Utilities {
                  def doSomething(): Unit = {
                    println("Hello")
                  }
                }
            "#
        },
        nav: "com/example/App.java", "<|>" => "Utilities.scala"
    }

    test java_references_scala_trait {
        files: {
            "com/example/App.java" => r"
                package com.example;

                public class App implements Configurab<|>le {
                }
            ",
            "com/example/Configurable.scala" => r"
                package com.example

                trait Configurable
            "
        },
        nav: "com/example/App.java", "<|>" => "Configurable.scala"
    }
}

lsp_test_suite! {
    name: "Scala Internal Navigation",

    test scala_class_reference {
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
        nav: "com/example/Main.scala", "<|>" => "User.scala"
    }

    test scala_method_call {
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
        nav: "com/example/Client.scala", "<|>" => "User.scala"
    }

    test scala_explicit_import_wins_over_same_simple_name {
        files: {
            "app/Main.scala" => r"
                package app
                import zed.Service
                object Main {
                  val service: Ser<|>vice = new Service()
                }
            ",
            "alpha/Service.scala" => "package alpha\nclass Service",
            "zed/Service.scala" => "package zed\nclass Service"
        },
        nav: "app/Main.scala", "<|>" => "zed/Service.scala"
    }

    test scala_same_package_type_wins_over_unrelated_simple_name {
        files: {
            "app/Main.scala" => r"
                package app
                object Main {
                  val service: Ser<|>vice = new Service()
                }
            ",
            "app/Service.scala" => "package app\nclass Service",
            "alpha/Service.scala" => "package alpha\nclass Service"
        },
        nav: "app/Main.scala", "<|>" => "app/Service.scala"
    }

    test scala_inferred_local_resolves_inherited_member {
        files: {
            "app/Main.scala" => r"
                package app
                object Main {
                  def run(): Unit = {
                    val child = new Child()
                    child.pi<|>ng()
                  }
                }
            ",
            "app/Child.scala" => "package app\nclass Child extends Base",
            "app/Base.scala" => "package app\nclass Base { def ping(): Unit = () }"
        },
        nav: "app/Main.scala", "<|>" => "app/Base.scala"
    }
}

lsp_goto_none_test! {
    name: scala_local_binding_does_not_leak_between_methods,
    files: {
        "app/Main.scala" => r"
            package app
            object Main {
              def first(): Unit = {
                val target = new Helper()
              }
              def second(): Unit = {
                target.pi<|>ng()
              }
            }
        ",
        "app/Helper.scala" => "package app\nclass Helper { def ping(): Unit = () }"
    },
    nav: "app/Main.scala", "<|>"
}

lsp_goto_none_test! {
    name: scala_ambiguous_unimported_type_is_not_guessed,
    files: {
        "app/Main.scala" => "package app\nobject Main { val service: Ser<|>vice = null }",
        "alpha/Service.scala" => "package alpha\nclass Service",
        "zed/Service.scala" => "package zed\nclass Service"
    },
    nav: "app/Main.scala", "<|>"
}
