// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Examples showing how to use test macros to simplify LSP tests
//!
//! Compare the verbose manual approach with the concise macro approach

// ============================================================================
// BEFORE: Manual approach (verbose, lots of boilerplate)
// ============================================================================

#[test]
fn test_method_call_on_field_manual() {
    use crate::lsp_test_framework::LspTest;
    use tower_lsp::lsp_types::GotoDefinitionResponse;

    let mut test = LspTest::new();

    test.add_file(
        "com/example/Foo.java",
        r"
        package com.example;
        import com.example.Bar;

        public class Foo {
            private Bar bar;
            public void test() {
                bar.ba<|>z();
            }
        }
    ",
    );

    test.add_file(
        "com/example/Bar.java",
        r#"
        package com.example;
        public class Bar {
            public void baz() {
                System.out.println("Hello");
            }
        }
    "#,
    );

    let result = test.goto_definition("com/example/Foo.java", "<|>");

    match result {
        Ok(Some(GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains("Bar.java"),
                "Expected file Bar.java, got: {}",
                loc.uri
            );
        }
        Ok(Some(GotoDefinitionResponse::Array(locs))) => {
            assert!(!locs.is_empty(), "Empty location array");
            assert!(
                locs[0].uri.as_str().contains("Bar.java"),
                "Expected file Bar.java, got: {}",
                locs[0].uri
            );
        }
        Ok(Some(GotoDefinitionResponse::Link(_))) => panic!("Unexpected link response"),
        Ok(None) => panic!("Expected definition, got None"),
        Err(e) => panic!("Error: {e}"),
    }
}

// ============================================================================
// AFTER: Using macros (clean, intention-revealing)
// ============================================================================

lsp_goto_test! {
    name: test_method_call_on_field_macro,
    files: {
        "com/example/Foo.java" => r"
            package com.example;
            import com.example.Bar;

            public class Foo {
                private Bar bar;
                public void test() {
                    bar.ba<|>z();
                }
            }
        ",

        "com/example/Bar.java" => r#"
            package com.example;
            public class Bar {
                public void baz() {
                    System.out.println("Hello");
                }
            }
        "#
    },
    nav: "com/example/Foo.java" ,  "<|>" => "Bar.java"
}

// ============================================================================
// COMPACT: For simple cases, even more concise
// ============================================================================

simple_goto_test!(
    test_param_navigation,
    files: {
        "Test.java" => "package com.example; class Test { void test(Bar b) { b.m<|>ethod(); } }",
        "Bar.java" => "package com.example; class Bar { void method() {} }"
    },
    "Test.java", "<|>" => "Bar.java"
);

// ============================================================================
// TEST SUITE: Group related tests
// ============================================================================

lsp_test_suite! {
    name: "Qualified Member Access Examples",

    test field_access {
        files: {
            "Foo.java" => r"
                package com.example;
                class Foo {
                    Bar bar;
                    void test() { bar.met<|>hod(); }
                }
            ",
            "Bar.java" => "package com.example; class Bar { void method() {} }"
        },
        nav: "Foo.java" ,  "<|>" => "Bar.java"
    }

    test local_var_access {
        files: {
            "Test.java" => r"
                package com.example;
                class Test {
                    void test() {
                        Bar bar = new Bar();
                        bar.met<|>hod();
                    }
                }
            ",
            "Bar.java" => "package com.example; class Bar { void method() {} }"
        },
        nav: "Test.java" ,  "<|>" => "Bar.java"
    }

    test parameter_access {
        files: {
            "Service.java" => r"
                package com.example;
                class Service {
                    void process(User user) {
                        user.getNa<|>me();
                    }
                }
            ",
            "User.java" => "package com.example; class User { String getName() { return \"\"; } }"
        },
        nav: "Service.java" ,  "<|>" => "User.java"
    }
}

// ============================================================================
// MULTI-CURSOR: Test multiple navigation points in one setup
// ============================================================================

multi_cursor_test! {
    name: test_chained_calls,

    files: {
        "Foo.java" => r"
            package com.example;
            class Foo {
                Bar bar;
                void test() {
                    bar.method/*c1*/One();
                    bar.method/*c2*/Two();
                    bar.method/*c3*/Three();
                }
            }
        ",
        "Bar.java" => r"
            package com.example;
            class Bar {
                void methodOne() {}
                void methodTwo() {}
                void methodThree() {}
            }
        "
    },

    checks: [
        "Foo.java", "c1" => "Bar.java",
        "Foo.java", "c2" => "Bar.java",
        "Foo.java", "c3" => "Bar.java"
    ]
}

// ============================================================================
// NEGATIVE TESTING: Expect no result
// ============================================================================

// NOTE: Local variable navigation now works! This test verifies that
// navigating to `count` goes to its declaration (same location when cursor is on definition).
lsp_goto_test! {
    name: test_local_variable_navigation,
    files: {
        "Test.java" => r"
            package com.example;
            public class Test {
                public void test() {
                    int count<|> = 0;
                }
            }
        "
    },
    nav: "Test.java", "<|>" => "Test.java"
}

// Test that primitive type keywords don't navigate anywhere
lsp_goto_none_test! {
    name: test_primitive_type_keyword,
    files: {
        "Test.java" => r"
            package com.example;
            public class Test {
                public void test() {
                    in<|>t count = 0;
                }
            }
        "
    },
    nav: "Test.java" ,  "<|>"
}

error_test! {
    name: test_undefined_method,
    files: {
        "Test.java" => r"
            package com.example;
            class Test {
                void test() {
                    Bar bar = new Bar();
                    bar.undefined<|>();
                }
            }
        ",

        "Bar.java" => "package com.example; class Bar { void defined() {} }"
    },
    nav: "Test.java" ,  "<|>" => expect_none
}

// ============================================================================
// CROSS-PACKAGE: With imports
// ============================================================================

lsp_goto_test! {
    name: test_nested_package,
    files: {
        "com/example/service/UserService.java" => r"
            package com.example.service;
            import com.example.model.User;

            public class UserService {
                public void process(User user) {
                    user.get<|>Id();
                }
            }
        ",

        "com/example/model/User.java" => r"
            package com.example.model;
            public class User {
                private Long id;
                public Long getId() { return id; }
            }
        "
    },
    nav: "com/example/service/UserService.java" ,  "<|>" => "model/User.java"
}

// ============================================================================
// WILDCARD IMPORT: Resolution with *
// ============================================================================

lsp_goto_test! {
    name: test_wildcard,
    files: {
        "com/example/Test.java" => r#"
            package com.example;
            import com.example.util.*;

            public class Test {
                public void test() {
                    StringUtils utils = new StringUtils();
                    utils.isEm<|>pty("");
                }
            }
        "#,

        "com/example/util/StringUtils.java" => r"
            package com.example.util;
            public class StringUtils {
                public boolean isEmpty(String s) {
                    return s == null || s.isEmpty();
                }
            }
        "
    },
    nav: "com/example/Test.java" ,  "<|>" => "StringUtils.java"
}

// ============================================================================
// BENEFITS OF MACRO APPROACH:
// ============================================================================
//
// 1. **Less Boilerplate**: ~60 lines → ~15 lines (75% reduction)
//
// 2. **More Readable**: Test intent is immediately clear
//    - See files involved at a glance
//    - See navigation expectation clearly
//    - No match statement clutter
//
// 3. **Easier to Write**: Just fill in the template
//    - Copy previous test
//    - Change file contents
//    - Change navigation expectation
//    - Done!
//
// 4. **Better Errors**: Macros generate helpful panic messages
//    - Shows expected vs actual file
//    - Includes test name in output
//    - Clear failure reasons
//
// 5. **Grouped Testing**: `lsp_test_suite!` runs related tests together
//    - Shows success rate
//    - Lists failed tests
//    - Better CI output
//
// 6. **Multi-Cursor**: Test multiple points without duplication
//    - One workspace setup
//    - Multiple checks
//    - Efficient and clear
//
// 7. **Type Safety**: Still fully type-checked by Rust
//    - Macros expand to safe code
//    - Compile-time guarantees
//    - No runtime overhead
