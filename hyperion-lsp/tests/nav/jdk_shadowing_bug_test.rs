// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test suite for JDK shadowing behavior
//
// Tests that workspace symbols properly shadow JDK classes when there are naming conflicts.
// For example, if the workspace defines `java.lang.String`, it should take precedence
// over the JDK's `java.lang.String`.
//
// Note: Unit tests in hyperion-workspace/src/index.rs provide more comprehensive coverage.
// These integration tests verify end-to-end LSP behavior.
lsp_test_suite! {
    name: "JDK Shadowing and Method References",

    test lambda_with_jdk_string_mock {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.function.Function;
                import java.lang.String;  // Explicit import of our mock
                public class Main {
                    public void test() {
                        Function<String, Integer> fn = s -> s.leng<|>th();
                    }
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public int length() { return 0; }
                }
            "
        },
        // Should navigate to our mock String.java, not fail because JDK class has no source
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test stream_with_custom_logger {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import com.example.util.Logger;  // Custom class, not JDK
                public class Main {
                    private Logger logger = new Logger();

                    public void test(List<String> list) {
                        list.forEach(logger::print<|>ln);
                    }
                }
            ",
            "com/example/util/Logger.java" => r"
                package com.example.util;
                public class Logger {
                    public void println(Object x) {}
                }
            "
        },
        // Should navigate to our custom Logger class
        nav: "com/example/Main.java", "<|>" => "Logger.java"
    }

    test constructor_call_with_jdk_mock {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.lang.Integer;
                public class Main {
                    public void test() {
                        Integer x = new Int<|>eger(42);
                    }
                }
            ",
            "java/lang/Integer.java" => r"
                package java.lang;
                public class Integer {
                    public Integer(int value) {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Integer.java"
    }
}

lsp_hover_test! {
    name: hover_on_overload_with_jdk_shadowing_prefers_call_site_arity,

    files: {
        "com/example/Main.java" => r"
            package com.example;
            import java.lang.String;

            public class Main {
                public void test() {
                    String s = new String();
                    s.tr<|>im(42);
                }
            }
        ",
        "java/lang/String.java" => r"
            package java.lang;
            public class String {
                public String trim() { return this; }
                public String trim(int flags) { return this; }
            }
        "
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["trim", "int"]
}
