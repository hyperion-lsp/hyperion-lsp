// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test navigation from method calls on foreach loop variables
lsp_test_suite! {
    name: "Foreach Loop Variable Navigation",

    // Test 1: Simple foreach with method call on loop variable
    test simple_foreach_method_call {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test() {
                        List<User> users = new ArrayList<>();
                        for (User u : users) {
                            u.getName<|>();
                        }
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;
                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Test 2: Foreach with explicit type (like the failing wildcard test)
    test foreach_explicit_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test(List<String> items) {
                        for (String s : items) {
                            s.length<|>();
                        }
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
}
