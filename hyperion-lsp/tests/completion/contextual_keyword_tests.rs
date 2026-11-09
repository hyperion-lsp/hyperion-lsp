// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// ============================================================================
// Contextual Keyword Tests
// ============================================================================
// These tests verify that contextual keywords like 'record', 'var', 'yield'
// are treated as identifiers (not keywords) in non-declaration contexts.

lsp_test_suite! {
    name: "Contextual Keywords as Parameters",

    // Test goto-definition on 'record' parameter
    test record_parameter_goto_definition {
        files: {
            "com/example/FeaturesDelta.java" => r#"
                package com.example;

                public class FeaturesDelta {
                    public void replay(FeatureLevelRecord record) {
                        if (rec<|>ord.name().equals("test")) {
                            System.out.println(record);
                        }
                    }
                }

                class FeatureLevelRecord {
                    public String name() { return ""; }
                }
            "#
        },
        nav: "com/example/FeaturesDelta.java", "<|>" => "FeaturesDelta.java"
    }

    // Test that goto-definition on parameter usage goes to parameter declaration
    test record_parameter_to_declaration {
        files: {
            "Test.java" => r"
                class Test {
                    void replay(Record record) {
                        System.out.println(rec<|>ord);
                    }
                }

                class Record {
                    String name;
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test var keyword as parameter name
    test var_as_parameter_name {
        files: {
            "Test.java" => r"
                class Test {
                    void process(String var) {
                        System.out.println(v<|>ar);
                    }
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test yield keyword as parameter name
    test yield_as_parameter_name {
        files: {
            "Test.java" => r"
                class Test {
                    void process(int yield) {
                        int result = yie<|>ld + 1;
                    }
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }
}

// Hover tests for contextual keywords
lsp_hover_test! {
    name: record_parameter_hover,

    files: {
        "com/example/FeaturesDelta.java" => r#"
            package com.example;

            public class FeaturesDelta {
                public void replay(FeatureLevelRecord record) {
                    if (rec<|>ord.name().equals("test")) {
                        System.out.println(record);
                    }
                }
            }

            class FeatureLevelRecord {
                public String name() { return ""; }
            }
        "#
    },

    hover: "com/example/FeaturesDelta.java", "<|>",
    contains: ["FeatureLevelRecord", "Parameter"]
}

lsp_hover_test! {
    name: var_parameter_hover,

    files: {
        "Test.java" => r"
            class Test {
                void process(String var) {
                    System.out.println(v<|>ar);
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["String", "Parameter"]
}

lsp_test_suite! {
    name: "Contextual Keywords - Variable Names",

    // Test 'record' as local variable name
    test record_local_variable {
        files: {
            "Test.java" => r"
                class Test {
                    void test() {
                        Record record = new Record();
                        System.out.println(rec<|>ord);
                    }
                }

                class Record {
                    String name;
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 'var' as local variable name
    test var_local_variable {
        files: {
            "Test.java" => r#"
                class Test {
                    void test() {
                        String var = "test";
                        System.out.println(v<|>ar);
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }
}

lsp_test_suite! {
    name: "Contextual Keywords - Field Names",

    // Test 'record' as field name
    test record_field_name {
        files: {
            "Test.java" => r"
                class Test {
                    private Record record;

                    void setRecord(Record record) {
                        this.rec<|>ord = record;
                    }
                }

                class Record {
                    String name;
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 'var' as field name
    test var_field_name {
        files: {
            "Test.java" => r"
                class Test {
                    private String var;

                    void setVar(String var) {
                        this.v<|>ar = var;
                    }
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }
}
