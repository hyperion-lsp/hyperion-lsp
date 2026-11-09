// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Smart Cast Completion Tests
//!
//! Tests for type narrowing after instanceof checks.
//! When a variable is checked with instanceof, the type should be narrowed
//! in the then-block for better completion suggestions.

// Test 1: Simple instanceof narrowing - Object to String
// After `if (obj instanceof String)`, obj should be narrowed to String
lsp_completion_test! {
    name: test_instanceof_simple_narrowing,
    files: {
        "Test.java" => r"
            class Test {
                void test(Object obj) {
                    if (obj instanceof String) {
                        obj.<|>
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["length", "charAt", "substring", "equals"]  // String methods
}

// Test 2: instanceof narrowing NOT applied outside then-block
lsp_completion_test! {
    name: test_instanceof_no_narrowing_outside_then,
    files: {
        "Test.java" => r"
            class Test {
                void test(Object obj) {
                    if (obj instanceof String) {
                        // narrowed here
                    }
                    obj.<|>
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["getClass", "hashCode", "toString"],  // Object methods
    not_contains: ["length", "charAt", "substring"]  // No String-specific methods
}

// Test 3: instanceof narrowing in else block should NOT apply
lsp_completion_test! {
    name: test_instanceof_no_narrowing_in_else,
    files: {
        "Test.java" => r"
            class Test {
                void test(Object obj) {
                    if (obj instanceof String) {
                        // narrowed here
                    } else {
                        obj.<|>
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["getClass", "hashCode", "toString"],  // Object methods only
    not_contains: ["length", "charAt"]  // No String methods
}

// Test 4: instanceof with && chain
lsp_completion_test! {
    name: test_instanceof_and_chain,
    files: {
        "Test.java" => r"
            class Test {
                void test(Object obj) {
                    if (obj instanceof String && obj.length() > 0) {
                        obj.<|>
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["length", "charAt", "substring"]  // String methods
}

// Test 5: instanceof narrowing for custom class
lsp_completion_test! {
    name: test_instanceof_custom_class,
    files: {
        "Test.java" => r"
            class Animal {
                public void eat() {}
            }
            class Dog extends Animal {
                public void bark() {}
                public void wagTail() {}
            }
            class Test {
                void test(Animal animal) {
                    if (animal instanceof Dog) {
                        animal.<|>
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["bark", "wagTail", "eat"]  // Dog methods + inherited Animal methods
}

// Test 6: Nested instanceof checks
lsp_completion_test! {
    name: test_instanceof_nested,
    files: {
        "Test.java" => r"
            class Test {
                void test(Object obj) {
                    if (obj instanceof String) {
                        if (obj.length() > 0) {
                            obj.<|>
                        }
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["length", "charAt", "substring"]  // Still narrowed to String in nested block
}

// Test 7: Pattern variable (Java 16+) - should show String methods
// Note: Pattern variables introduce a new variable, not narrow existing one
lsp_completion_test! {
    name: test_pattern_variable_completion,
    files: {
        "Test.java" => r"
            class Test {
                void test(Object obj) {
                    if (obj instanceof String s) {
                        s.<|>
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["length", "charAt", "substring"]  // String methods on pattern variable
}

// Test 8: instanceof to array type
lsp_completion_test! {
    name: test_instanceof_array_type,
    files: {
        "Test.java" => r"
            class Test {
                void test(Object obj) {
                    if (obj instanceof String[]) {
                        obj.<|>
                    }
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["length", "clone"]  // Array properties
}
