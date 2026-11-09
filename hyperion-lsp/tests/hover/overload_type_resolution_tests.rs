// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for type-based method overload resolution
//
// These tests verify that goto-definition navigates to the correct
// method overload based on argument TYPES, not just argument count.
lsp_test_suite! {
    name: "Type-Based Overload Resolution",

    // Basic type differentiation: int vs String with same param count
    test int_vs_string_same_count {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    void <target>process(int n) {}
                    void process(String s) {}

                    void test() {
                        proc<|>ess(42);
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // String argument matches String parameter
    test string_arg_matches_string_param {
        files: {
            "com/example/Test.java" => r#"
                package com.example;

                public class Test {
                    void process(int n) {}
                    void <target>process(String s) {}

                    void test() {
                        proc<|>ess("hello");
                    }
                }
            "#
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // Boolean literal matches boolean parameter
    test boolean_arg_matches_boolean_param {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    void process(int n) {}
                    void <target>process(boolean b) {}

                    void test() {
                        proc<|>ess(true);
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // Long literal matches long parameter (not int)
    test long_literal_matches_long_param {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    void process(int n) {}
                    void <target>process(long l) {}

                    void test() {
                        proc<|>ess(42L);
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // Double literal matches double parameter
    test double_literal_matches_double_param {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    void process(int n) {}
                    void <target>process(double d) {}

                    void test() {
                        proc<|>ess(3.14);
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // new expression type inference
    test new_expression_type {
        files: {
            "com/example/User.java" => r"
                package com.example;
                public class User {}
            ",
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    void <target>process(User u) {}
                    void process(String s) {}

                    void test() {
                        proc<|>ess(new User());
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // Exact match preferred over widening
    test exact_match_preferred_over_widening {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    void <target>process(int n) {}
                    void process(long l) {}

                    void test() {
                        proc<|>ess(42);
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // Null matches reference type, not primitive
    test null_matches_reference_type {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    void process(int n) {}
                    void <target>process(String s) {}

                    void test() {
                        proc<|>ess(null);
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // Multiple args with mixed types
    test multiple_args_mixed_types {
        files: {
            "com/example/Test.java" => r#"
                package com.example;

                public class Test {
                    void process(String s, int n) {}
                    void <target>process(int n, String s) {}

                    void test() {
                        proc<|>ess(42, "hello");
                    }
                }
            "#
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // Cast expression type
    test cast_expression_type {
        files: {
            "com/example/Test.java" => r#"
                package com.example;

                public class Test {
                    void <target>process(String s) {}
                    void process(int n) {}

                    void test() {
                        Object obj = "hello";
                        proc<|>ess((String) obj);
                    }
                }
            "#
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }
}

lsp_test_suite! {
    name: "Subtype Cast Overload Resolution",

    test cast_implementer_selects_interface_overload {
        files: {
            "demo/Test.java" => r"
                package demo;
                interface Task {}
                class Child implements Task {}
                class Test {
                    void accept(Object value) {}
                    void <target>accept(Task value) {}
                    void run(Object value) { acc<|>ept((Child) value); }
                }
            "
        },
        nav: "demo/Test.java", "<|>" => "<target>"
    }

    test bounded_parameter_selects_interface_overload {
        files: {
            "demo/Test.java" => r"
                package demo;
                interface Task {}
                class Base {}
                class Test {
                    void accept(Object value) {}
                    void <target>accept(Task value) {}
                    <T extends Base & Task> void run(T value) { acc<|>ept(value); }
                }
            "
        },
        nav: "demo/Test.java", "<|>" => "<target>"
    }
}

#[test]
fn cast_implementer_method_hover_matches_navigation() {
    let mut test = crate::lsp_test_framework::LspTest::new();
    test.add_file(
        "demo/Test.java",
        r"
        package demo;
        interface Task {}
        class Child implements Task {}
        class Result {}
        class Sink {
            Result accept(Task value) { return null; }
            int accept(Object value) { return 0; }
        }
        class Test {
            void run(Sink sink, Object object) {
                Result first = sink.acc<|>ept((Child) object);
            }
        }
    ",
    );
    let hover = test
        .hover("demo/Test.java", "<|>")
        .unwrap()
        .expect("method hover");
    let rendered = format!("{:?}", hover.contents);
    assert!(rendered.contains("Task"), "{rendered}");
    assert!(rendered.contains("Result"), "{rendered}");
}
