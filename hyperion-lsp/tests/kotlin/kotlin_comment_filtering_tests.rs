// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for comment/string filtering in Kotlin LSP navigation
//!
//! Verifies that LSP features (goto-definition, hover, find-references, completion)
//! do not work when the cursor is inside comments or string literals.

// Test 1: Goto-definition should return None when cursor is in line comment
lsp_goto_none_test! {
    name: goto_definition_in_line_comment,
    files: {
        "Test.kt" => r"
            package com.example

            fun test() {
                val variable = 42
                // This mentions vari<|>able in a comment
            }
        "
    },
    nav: "Test.kt", "<|>"
}

// Test 2: Goto-definition should return None when cursor is in block comment
// Using caret marker (^) to point to location in comment above
lsp_goto_none_test! {
    name: goto_definition_in_block_comment,
    files: {
        "Test.kt" => r"
            package com.example

            fun test() {
                val variable = 42
                /* This is a block comment about variable */
                ^
            }
        "
    },
    nav: "Test.kt", "^"
}

// Test 3: Goto-definition should return None when cursor is in string literal
lsp_goto_none_test! {
    name: goto_definition_in_string_literal,
    files: {
        "Test.kt" => r#"
            package com.example

            fun test() {
                val variable = 42
                val message = "The vari<|>able is set"
            }
        "#
    },
    nav: "Test.kt", "<|>"
}

// Test 4: Goto-definition should return None when cursor is in raw string literal
lsp_goto_none_test! {
    name: goto_definition_in_raw_string,
    files: {
        "Test.kt" => r#"
            package com.example

            fun test() {
                val variable = 42
                val message = """
                    Multi-line string with vari<|>able reference
                """
            }
        "#
    },
    nav: "Test.kt", "<|>"
}

// Test 5: Goto-definition should return None when cursor is in multiline comment
// Using caret marker (^) to point to location in comment above
lsp_goto_none_test! {
    name: goto_definition_in_multiline_comment,
    files: {
        "Test.kt" => r"
            package com.example

            fun test() {
                val variable = 42
                /*
                 * This is a multiline
                 * comment mentioning variable
                 */
                ^
            }
        "
    },
    nav: "Test.kt", "^"
}

// Test 6: Goto-definition SHOULD work when cursor is on actual code (not in comment)
lsp_goto_test! {
    name: goto_definition_works_in_code,
    files: {
        "Test.kt" => r"
            package com.example

            fun test() {
                val variable = 42
                // Comment above
                println(vari<|>able)  // This is actual code
            }
        "
    },
    nav: "Test.kt", "<|>" => "Test.kt"
}

// Test 7: Goto-definition on class name in comment should return None
lsp_goto_none_test! {
    name: goto_definition_class_in_comment,
    files: {
        "User.kt" => r#"
            package com.example

            class User {
                val name: String = ""
            }
        "#,
        "Test.kt" => r"
            package com.example

            fun test() {
                // This mentions Use<|>r class
                val user = User()
            }
        "
    },
    nav: "Test.kt", "<|>"
}

// Test 8: Goto-definition on function name in string should return None
lsp_goto_none_test! {
    name: goto_definition_function_in_string,
    files: {
        "Test.kt" => r#"
            package com.example

            fun myFunction() {
                println("Calling myFunc<|>tion")
            }
        "#
    },
    nav: "Test.kt", "<|>"
}
