// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Same-package navigation resolves without an import
// (no explicit import is needed for classes in the same package)

lsp_goto_test! {
    name: verify_same_package_works,
    files: {
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                public void test() {
                    Us<|>er user = new User();
                }
            }
        ",
        "com/example/User.java" => r"
            package com.example;

            public class User {
                private String name;
            }
        "
    },
    nav: "com/example/Main.java", "<|>" => "User.java"
}
