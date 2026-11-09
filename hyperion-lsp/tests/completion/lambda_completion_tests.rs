// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Lambda Completion Tests
//!
//! Tests for lambda expression completions in functional interface contexts.

// Test 1: forEach with Consumer
lsp_completion_test! {
    name: test_foreach_lambda,
    files: {
        "Test.java" => r"
            import java.util.List;
            class Test {
                void test(List<String> names) {
                    names.forEach(<|>)
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["t -> ", "t -> {}"]
}

// Test 2: map with Function
lsp_completion_test! {
    name: test_map_lambda,
    files: {
        "Test.java" => r"
            import java.util.stream.Stream;
            class Test {
                void test(Stream<String> stream) {
                    stream.map(<|>)
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["t -> ", "t -> {}"]
}

// Test 3: filter with Predicate
lsp_completion_test! {
    name: test_filter_lambda,
    files: {
        "Test.java" => r"
            import java.util.stream.Stream;
            class Test {
                void test(Stream<Integer> stream) {
                    stream.filter(<|>)
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["t -> ", "t -> {}"]
}

// Test 4: sorted with Comparator (2 parameters)
lsp_completion_test! {
    name: test_sorted_lambda,
    files: {
        "Test.java" => r"
            import java.util.stream.Stream;
            class Test {
                void test(Stream<String> stream) {
                    stream.sorted(<|>)
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["(t, t2) -> ", "(t, t2) -> {}"]
}

// Test 5: removeIf with Predicate
lsp_completion_test! {
    name: test_removeif_lambda,
    files: {
        "Test.java" => r"
            import java.util.List;
            class Test {
                void test(List<String> list) {
                    list.removeIf(<|>)
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["t -> "]
}
