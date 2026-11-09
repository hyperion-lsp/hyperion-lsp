// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Minimal reproduction tests for identified bugs
// These tests should FAIL until the bugs are fixed

// Method call on cast expression
lsp_test_suite! {
    name: "Bug1 Cast Expression",

    test simple_cast_method_call {
        files: {
            "Test.java" => r"
                package test;

                interface Runnable {
                    void run();
                }

                class Test {
                    void test(Object obj) {
                        ((Runnable) obj).r<|>un();
                    }
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    test cast_with_array_access {
        files: {
            "Test.java" => r"
                package test;

                interface Type {
                    String getName();
                }

                class Test {
                    void test(Object type) {
                        ((Type[]) type)[0].getN<|>ame();
                    }
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    test imported_cast_with_array_access {
        files: {
            "test/Test.java" => r"
                package test;

                import api.Type;

                class Test {
                    void test(Object type) {
                        ((Type[]) type)[0].getN<|>ame();
                    }
                }
            ",
            "api/Type.java" => r"
                package api;

                public interface Type {
                    String getName();
                }
            "
        },
        nav: "test/Test.java", "<|>" => "Type.java"
    }
}

// Method call on array element
lsp_test_suite! {
    name: "Bug2 Array Element",

    test simple_array_element_method {
        files: {
            "Test.java" => r#"
                package test;

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    void test(User[] users) {
                        String name = users[0].getN<|>ame();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    test array_element_from_variable {
        files: {
            "Test.java" => r"
                package test;

                interface Type {
                    boolean equals(Object other);
                }

                class Test {
                    void test(Type[] types) {
                        boolean result = types[0].equ<|>als(types[1]);
                    }
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    test array_element_cross_file {
        files: {
            "test/Test.java" => r"
                package test;

                import java.lang.reflect.Type;

                class Test {
                    void test(Type[] types, Type[] otherTypes) {
                        boolean equal = types[0].equ<|>als(otherTypes[0]);
                    }
                }
            ",
            "java/lang/reflect/Type.java" => r"
                package java.lang.reflect;

                public interface Type {
                    boolean equals(Object other);
                }
            "
        },
        nav: "test/Test.java", "<|>" => "Type.java"
    }

    test nested_array_access {
        files: {
            "Test.java" => r#"
                package test;

                class Matrix {
                    String getValue() { return ""; }
                }

                class Test {
                    void test(Matrix[][] matrix) {
                        String val = matrix[0][0].getV<|>alue();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }
}
