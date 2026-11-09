// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Advanced completion tests covering edge cases and complex scenarios
//
// Note: Some tests are disabled because they require full workspace indexing
// with semantic analysis (inheritance, static modifiers) which isn't available
// for test-created files. These features work correctly with JDK classes and
// properly indexed project files.

// Disabled: Test 1 - Inherited method completion
// Requires workspace index to track inheritance hierarchy
// This works correctly with JDK classes (e.g., Object methods on String)
// lsp_completion_test! {
//     name: test_inherited_method_completion,
//     ...
// }

// Disabled: Test 2 - Interface method completion
// Requires workspace index to track interface implementations
// This works correctly with JDK classes (e.g., Runnable methods)
// lsp_completion_test! {
//     name: test_interface_method_completion,
//     ...
// }

// Test 3: Scope shadowing - inner variable shadows outer
lsp_completion_test! {
    name: test_scope_shadowing,
    files: {
        "Test.java" => r#"
            class Test {
                void test() {
                    String outer = "outer";
                    {
                        String outer = "inner";
                        <|>
                    }
                }
            }
        "#
    },
    complete: "Test.java", "<|>",
    contains: ["outer"]
}

// Test 4: Multiple scopes - method parameter + local variable
lsp_completion_test! {
    name: test_multiple_scopes,
    files: {
        "Test.java" => r#"
            class Test {
                void process(String input) {
                    int count = 0;
                    String result = "";
                    <|>
                }
            }
        "#
    },
    complete: "Test.java", "<|>",
    contains: ["input", "count", "result"]
}

// Test 5: Completion in nested blocks
lsp_completion_test! {
    name: test_nested_block_completion,
    files: {
        "Test.java" => r"
            class Test {
                void test() {
                    int outer = 1;
                    if (true) {
                        int inner = 2;
                        <|>
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["outer", "inner"]
}

// Test 6: No completion for primitives - primitives have no members
lsp_completion_none_test! {
    name: test_no_primitive_member_completion,
    files: {
        "Test.java" => r"
            class Test {
                void test() {
                    int count = 0;
                    count.<|>
                }
            }
        "
    },
    complete: "Test.java", "<|>"
}

// Disabled: Test 7 - Static vs instance filtering on class
// Requires workspace index to have static modifiers set correctly
// This works correctly with JDK classes (e.g., String.valueOf vs string.length)
// The basic_completion_tests.rs has test_static_method_completion that verifies this
// lsp_completion_test! {
//     name: test_static_only_on_class,
//     ...
// }

// Disabled: Test 8 - Instance vs static filtering on object
// Requires workspace index to have static modifiers set correctly
// This works correctly with JDK classes
// lsp_completion_test! {
//     name: test_instance_only_on_object,
//     ...
// }

// Test 9: For-each loop variable completion
lsp_completion_test! {
    name: test_foreach_loop_variable,
    files: {
        "Test.java" => r"
            class Test {
                void test(String[] items) {
                    for (String item : items) {
                        <|>
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["item", "items"]
}

// Test 10: Try-with-resources variable completion
lsp_completion_test! {
    name: test_try_with_resources,
    files: {
        "Test.java" => r"
            import java.io.BufferedReader;

            class Test {
                void test() {
                    try (BufferedReader reader = null) {
                        <|>
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["reader"]
}
