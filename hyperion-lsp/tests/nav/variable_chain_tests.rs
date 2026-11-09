// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Minimal tests to isolate variable-based method chaining issues

lsp_test_suite! {
    name: "Variable-Based Method Chaining",

    // MINIMAL: Single method call on a variable
    test variable_single_method {
        files: {
            "Test.java" => r"
                class Test {
                    void test() {
                        Builder b = new Builder();
                        b.build<|>();
                    }
                }
            ",
            "Builder.java" => r"
                class Builder {
                    void build() {}
                }
            "
        },
        nav: "Test.java", "<|>" => "Builder.java"
    }

    // MINIMAL: Two methods chained on a variable
    test variable_two_method_chain {
        files: {
            "Test.java" => r#"
                class Test {
                    void test() {
                        Builder b = new Builder();
                        b.setName("x").build<|>();
                    }
                }
            "#,
            "Builder.java" => r"
                class Builder {
                    Builder setName(String s) { return this; }
                    void build() {}
                }
            "
        },
        nav: "Test.java", "<|>" => "Builder.java"
    }

    // MINIMAL: Three methods chained on a variable (like the failing test)
    test variable_three_method_chain {
        files: {
            "Test.java" => r#"
                class Test {
                    void test() {
                        Builder b = new Builder();
                        b.setName("x").setEmail("y").build<|>();
                    }
                }
            "#,
            "Builder.java" => r"
                class Builder {
                    Builder setName(String s) { return this; }
                    Builder setEmail(String s) { return this; }
                    void build() {}
                }
            "
        },
        nav: "Test.java", "<|>" => "Builder.java"
    }
}
