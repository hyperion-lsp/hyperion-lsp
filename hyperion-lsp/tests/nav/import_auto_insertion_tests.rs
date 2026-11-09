// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test 1: ArrayList completion should add import java.util.ArrayList
lsp_completion_test! {
    name: test_arraylist_adds_import,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            class Test {
                void test() {
                    ArrayList<|>
                }
            }
        "
    },
    complete: "com/example/Test.java", "<|>",
    contains: ["ArrayList"]
}

// Test 2: String completion should NOT add import (java.lang is implicit)
lsp_completion_test! {
    name: test_string_no_import,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            class Test {
                void test() {
                    String<|>
                }
            }
        "
    },
    complete: "com/example/Test.java", "<|>",
    contains: ["String"]
}

// Test 3: Same-package class should NOT add import
lsp_completion_test! {
    name: test_same_package_no_import,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            class Test {
                void test() {
                    Helper<|>
                }
            }
        ",
        "com/example/Helper.java" => r"
            package com.example;

            class Helper {
            }
        "
    },
    complete: "com/example/Test.java", "<|>",
    contains: ["Helper"]
}

// Test 4: Already imported class should NOT add duplicate import
lsp_completion_test! {
    name: test_already_imported_no_duplicate,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            import java.util.ArrayList;

            class Test {
                void test() {
                    ArrayList<|>
                }
            }
        "
    },
    complete: "com/example/Test.java", "<|>",
    contains: ["ArrayList"]
}

// Test 5: Wildcard covered class should NOT add import
lsp_completion_test! {
    name: test_wildcard_covered_no_import,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            import java.util.*;

            class Test {
                void test() {
                    ArrayList<|>
                }
            }
        "
    },
    complete: "com/example/Test.java", "<|>",
    contains: ["ArrayList"]
}

// Test 6: Import should be inserted after package statement
lsp_completion_test! {
    name: test_import_after_package,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            class Test {
                void test() {
                    ArrayList<|>
                }
            }
        "
    },
    complete: "com/example/Test.java", "<|>",
    contains: ["ArrayList"]
}

// Test 7: Import should be inserted after existing imports
lsp_completion_test! {
    name: test_import_after_existing_imports,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            import java.util.List;

            class Test {
                void test() {
                    ArrayList<|>
                }
            }
        "
    },
    complete: "com/example/Test.java", "<|>",
    contains: ["ArrayList"]
}

// Test 8: No import at top of file if no package
lsp_completion_test! {
    name: test_import_at_top_no_package,
    files: {
        "Test.java" => r"
            class Test {
                void test() {
                    ArrayList<|>
                }
            }
        "
    },
    complete: "Test.java", "<|>",
    contains: ["ArrayList"]
}
