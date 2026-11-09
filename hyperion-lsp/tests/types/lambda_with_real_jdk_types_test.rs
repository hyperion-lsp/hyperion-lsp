// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Verify that the ORIGINAL lambda tests work with actual java.lang.String
// These tests should now pass after fixing the JDK shadowing bug
lsp_test_suite! {
    name: "Lambda Tests with Real JDK Types",

    test basic_lambda_with_real_string {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.function.Function;
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
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test stream_map_with_real_string {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test() {
                        List<String> names = Arrays.asList("a", "b");
                        List<Integer> lengths = names.stream()
                            .map(s -> s.leng<|>th())
                            .collect(Collectors.toList());
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public int length() { return 0; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }
}
