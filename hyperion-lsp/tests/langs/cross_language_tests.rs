// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Cross-language integration tests
//
// These tests verify navigation between different JVM languages:
// - Java ↔ Kotlin
// - Java ↔ Groovy
// - Kotlin ↔ Groovy

lsp_test_suite! {
    name: "Java to Kotlin Navigation",

    test java_uses_kotlin_class {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void run() {
                        Kotlin<|>User user = new KotlinUser();
                    }
                }
            ",
            "com/example/KotlinUser.kt" => r#"
                package com.example

                class KotlinUser {
                    val name: String = ""
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "KotlinUser.kt"
    }

    test java_calls_kotlin_function {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void run() {
                        Kotlin<|>Utils.greet("World");
                    }
                }
            "#,
            "com/example/KotlinUtils.kt" => r#"
                package com.example

                object KotlinUtils {
                    fun greet(name: String) {
                        println("Hello, $name")
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "KotlinUtils.kt"
    }
}

lsp_test_suite! {
    name: "Kotlin to Java Navigation",

    test kotlin_uses_java_class {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val user: Java<|>User = JavaUser()
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
        nav: "com/example/Main.kt", "<|>" => "JavaUser.java"
    }

    test kotlin_calls_java_method {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val user = JavaUser()
                    user.get<|>Name()
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
        nav: "com/example/Main.kt", "<|>" => "JavaUser.java"
    }
}

lsp_test_suite! {
    name: "Java to Groovy Navigation",

    test java_uses_groovy_class {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void run() {
                        Groovy<|>User user = new GroovyUser();
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

    test java_calls_groovy_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void run() {
                        GroovyHelper helper = new GroovyHelper();
                        helper.hel<|>p();
                    }
                }
            ",
            "com/example/GroovyHelper.groovy" => r#"
                package com.example

                class GroovyHelper {
                    void help() {
                        println "Helping!"
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "GroovyHelper.groovy"
    }
}

lsp_test_suite! {
    name: "Groovy to Java Navigation",

    test groovy_uses_java_class {
        files: {
            "com/example/Main.groovy" => r"
                package com.example

                class Main {
                    void run() {
                        Java<|>Service service = new JavaService()
                    }
                }
            ",
            "com/example/JavaService.java" => r#"
                package com.example;

                public class JavaService {
                    public void process() {
                        System.out.println("Processing");
                    }
                }
            "#
        },
        nav: "com/example/Main.groovy", "<|>" => "JavaService.java"
    }

    test groovy_calls_java_method {
        files: {
            "com/example/Main.groovy" => r"
                package com.example

                class Main {
                    void run() {
                        def service = new JavaService()
                        service.proc<|>ess()
                    }
                }
            ",
            "com/example/JavaService.java" => r#"
                package com.example;

                public class JavaService {
                    public void process() {
                        System.out.println("Processing");
                    }
                }
            "#
        },
        nav: "com/example/Main.groovy", "<|>" => "JavaService.java"
    }
}

lsp_test_suite! {
    name: "Kotlin to Groovy Navigation",

    test kotlin_uses_groovy_class {
        files: {
            "com/example/Main.kt" => r"
                package com.example

                fun main() {
                    val config: Groovy<|>Config = GroovyConfig()
                }
            ",
            "com/example/GroovyConfig.groovy" => r#"
                package com.example

                class GroovyConfig {
                    String name = ""
                }
            "#
        },
        nav: "com/example/Main.kt", "<|>" => "GroovyConfig.groovy"
    }
}

lsp_test_suite! {
    name: "Groovy to Kotlin Navigation",

    test groovy_uses_kotlin_class {
        files: {
            "com/example/Main.groovy" => r"
                package com.example

                class Main {
                    void run() {
                        Kotlin<|>Data data = new KotlinData()
                    }
                }
            ",
            "com/example/KotlinData.kt" => r#"
                package com.example

                data class KotlinData(
                    val name: String = ""
                )
            "#
        },
        nav: "com/example/Main.groovy", "<|>" => "KotlinData.kt"
    }
}

lsp_test_suite! {
    name: "Three-Way Navigation",

    test java_kotlin_groovy_chain {
        files: {
            "com/example/JavaEntry.java" => r"
                package com.example;

                public class JavaEntry {
                    public void run() {
                        Kotlin<|>Bridge bridge = new KotlinBridge();
                    }
                }
            ",
            "com/example/KotlinBridge.kt" => r"
                package com.example

                class KotlinBridge {
                    fun connect(): GroovyBackend = GroovyBackend()
                }
            ",
            "com/example/GroovyBackend.groovy" => r#"
                package com.example

                class GroovyBackend {
                    void process() {
                        println "Processing in Groovy"
                    }
                }
            "#
        },
        nav: "com/example/JavaEntry.java", "<|>" => "KotlinBridge.kt"
    }
}
