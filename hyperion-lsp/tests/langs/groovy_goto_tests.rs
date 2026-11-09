// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Goto-definition tests for Groovy language support
//
// These tests verify that goto-definition works for Groovy files, including:
// - Cross-file navigation (class, method, property references)
// - Same-file navigation (local variables, parameters, etc.)
// - Cross-language navigation (Groovy -> Java, Java -> Groovy)

lsp_test_suite! {
    name: "Groovy Cross-File Navigation",

    test groovy_class_reference {
        files: {
            "com/example/Main.groovy" => r"
                package com.example

                class Main {
                    void run() {
                        Us<|>er user = new User()
                    }
                }
            ",
            "com/example/User.groovy" => r"
                package com.example

                class User {
                    String name
                }
            "
        },
        nav: "com/example/Main.groovy", "<|>" => "User.groovy"
    }

    test groovy_method_call {
        files: {
            "com/example/Main.groovy" => r#"
                package com.example

                class Main {
                    void run() {
                        def helper = new Helper()
                        helper.gre<|>et("World")
                    }
                }
            "#,
            "com/example/Helper.groovy" => r#"
                package com.example

                class Helper {
                    void greet(String name) {
                        println "Hello, $name"
                    }
                }
            "#
        },
        nav: "com/example/Main.groovy", "<|>" => "Helper.groovy"
    }

    test groovy_property_access {
        files: {
            "com/example/Main.groovy" => r"
                package com.example

                class Main {
                    void run() {
                        def user = new User()
                        println user.na<|>me
                    }
                }
            ",
            "com/example/User.groovy" => r#"
                package com.example

                class User {
                    String name = "John"
                }
            "#
        },
        nav: "com/example/Main.groovy", "<|>" => "User.groovy"
    }
}

lsp_test_suite! {
    name: "Groovy Resolution Semantics",

    test groovy_explicit_import_wins_over_same_simple_name {
        files: {
            "app/Main.groovy" => r"
                package app
                import zed.Service

                class Main {
                    Ser<|>vice service
                }
            ",
            "alpha/Service.groovy" => "package alpha\nclass Service {}",
            "zed/Service.groovy" => "package zed\nclass Service {}"
        },
        nav: "app/Main.groovy", "<|>" => "zed/Service.groovy"
    }

    test groovy_same_package_type_wins_over_unrelated_simple_name {
        files: {
            "app/Main.groovy" => r"
                package app
                class Main {
                    Ser<|>vice service
                }
            ",
            "app/Service.groovy" => "package app\nclass Service {}",
            "alpha/Service.groovy" => "package alpha\nclass Service {}"
        },
        nav: "app/Main.groovy", "<|>" => "app/Service.groovy"
    }

    test groovy_inherited_safe_member_access {
        files: {
            "app/Main.groovy" => r"
                package app
                class Main {
                    void run() {
                        def child = new Child()
                        child?.pi<|>ng()
                    }
                }
            ",
            "app/Child.groovy" => "package app\nclass Child extends Base {}",
            "app/Base.groovy" => "package app\nclass Base { void ping() {} }"
        },
        nav: "app/Main.groovy", "<|>" => "app/Base.groovy"
    }

    test groovy_default_import_resolves_java_util {
        files: {
            "app/Main.groovy" => r"
                package app
                class Main {
                    Li<|>st values
                }
            ",
            "java/util/List.java" => "package java.util; public interface List {}"
        },
        nav: "app/Main.groovy", "<|>" => "java/util/List.java"
    }
}

lsp_goto_none_test! {
    name: groovy_local_binding_does_not_leak_between_methods,
    files: {
        "app/Main.groovy" => r"
            package app
            class Main {
                void first() {
                    def target = new Helper()
                }
                void second() {
                    target.pi<|>ng()
                }
            }
        ",
        "app/Helper.groovy" => "package app\nclass Helper { void ping() {} }"
    },
    nav: "app/Main.groovy", "<|>"
}

lsp_test_suite! {
    name: "Groovy-Java Cross-Language Navigation",

    test groovy_to_java_class {
        files: {
            "com/example/Main.groovy" => r"
                package com.example

                class Main {
                    void run() {
                        JavaUs<|>er user = new JavaUser()
                    }
                }
            ",
            "com/example/JavaUser.java" => r"
                package com.example;

                public class JavaUser {
                    private String name;

                    public String getName() {
                        return name;
                    }
                }
            "
        },
        nav: "com/example/Main.groovy", "<|>" => "JavaUser.java"
    }

    test java_to_groovy_class {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void run() {
                        GroovyUs<|>er user = new GroovyUser();
                    }
                }
            ",
            "com/example/GroovyUser.groovy" => r"
                package com.example

                class GroovyUser {
                    String name
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "GroovyUser.groovy"
    }
}

// Gradle DSL methods are not yet resolvable - should not panic
lsp_goto_none_test! {
    name: gradle_dependency_method,
    files: {
        "build.gradle" => r"
            dependencies {
                implementa<|>tion 'com.google.guava:guava:31.0-jre'
            }
        "
    },
    nav: "build.gradle", "<|>"
}
