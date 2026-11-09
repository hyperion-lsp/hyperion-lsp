// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Goto-definition tests for pattern variables (Java 16+)

// ============================================================================
// Basic Pattern Variable Navigation
// ============================================================================

lsp_test_suite! {
    name: "Pattern Variable Goto Definition",

    // Test 1: Navigate from pattern variable usage to its declaration
    test simple_pattern_variable {
        files: {
            "com/example/Test.java" => r"
                package com.example;

                public class Test {
                    void method(Object obj) {
                        if (obj instanceof String s) {
                            System.out.println(s<|>.toUpperCase());
                        }
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "Test.java"
    }

    // Test 2: Navigate from pattern variable in nested block
    test pattern_variable_in_nested_block {
        files: {
            "com/example/Handler.java" => r"
                package com.example;

                class Handler {
                    void process(Object obj) {
                        try {
                            if (obj instanceof String str) {
                                if (str<|>.length() > 0) {
                                    System.out.println(str);
                                }
                            }
                        } catch (Exception e) {
                        }
                    }
                }
            "
        },
        nav: "com/example/Handler.java", "<|>" => "Handler.java"
    }

    // Test 3: Pattern variable with custom type
    test pattern_variable_with_custom_type {
        files: {
            "com/example/Processor.java" => r"
                package com.example;
                import com.example.model.User;

                public class Processor {
                    void handle(Object obj) {
                        if (obj instanceof User user) {
                            String name = user<|>.getName();
                        }
                    }
                }
            ",

            "com/example/model/User.java" => r"
                package com.example.model;

                public class User {
                    private String name;
                    public String getName() { return name; }
                }
            "
        },
        nav: "com/example/Processor.java", "<|>" => "Processor.java"
    }
}

// ============================================================================
// Pattern Variables in Complex Scenarios
// ============================================================================

lsp_test_suite! {
    name: "Complex Pattern Variable Scenarios",

    // Test 2: Multiple pattern variables in same method
    test multiple_pattern_variables {
        files: {
            "com/example/Multi.java" => r"
                package com.example;

                class Multi {
                    void process(Object obj1, Object obj2) {
                        if (obj1 instanceof String s1) {
                            System.out.println(s1<|>.length());
                        }
                        if (obj2 instanceof String s2) {
                            System.out.println(s2.length());
                        }
                    }
                }
            "
        },
        nav: "com/example/Multi.java", "<|>" => "Multi.java"
    }
}
