// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_goto_test! {
    name: basic_parameter_gd,
    files: {
        "Test.java" => r"
            class Test {
                void test(String param) {
                    System.out.println(par<|>am);
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}
