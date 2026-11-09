// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Negative Test Cases for LSP Goto-Definition
//!
//! These tests verify the LSP correctly returns no definition for:
//! - Undefined/non-existent symbols
//! - Primitive types and keywords
//! - Invalid import scenarios
//! - Parse error contexts
//! - Type system edge cases

// ============================================================================
// Core Language - Undefined Symbols (5 tests)
// ============================================================================

// Navigate to non-existent class - should return None
lsp_goto_none_test! {
    name: undefined_class_reference,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test() {
                    Unknown<|>Class obj = null;
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Navigate to non-existent method - should return None
lsp_goto_none_test! {
    name: undefined_method_call,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                private String name;

                public String getName() {
                    return name;
                }
            }
        ",
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test() {
                    User user = new User();
                    user.unknown<|>Method();
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Navigate to non-existent field - should return None
lsp_goto_none_test! {
    name: undefined_field_access,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                private String name;
            }
        ",
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test() {
                    User user = new User();
                    String x = user.nonexistent<|>Field;
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Navigate on primitive type keyword - should return None
lsp_goto_none_test! {
    name: primitive_type_keyword,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test() {
                    in<|>t count = 0;
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Navigate on undefined variable usage - should return None
lsp_goto_none_test! {
    name: undefined_variable_usage,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test() {
                    int result = undeclared<|>Variable + 1;
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// ============================================================================
// Import Issues (3 tests)
// ============================================================================

// Static import of non-existent method - should return None
lsp_goto_none_test! {
    name: invalid_static_import,
    files: {
        "com/example/Util.java" => r"
            package com.example;

            public class Util {
                public static void existingMethod() {}
            }
        ",
        "com/example/Test.java" => r"
            package com.example;

            import static com.example.Util.nonExistent<|>Method;

            public class Test {
                public void test() {
                    nonExistentMethod();
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Two wildcard imports with potential ambiguous class - should return None
// when the referenced class doesn't exist in either package
lsp_goto_none_test! {
    name: wildcard_ambiguous,
    files: {
        "com/alpha/User.java" => r"
            package com.alpha;

            public class User {
                public String name;
            }
        ",
        "com/beta/Admin.java" => r"
            package com.beta;

            public class Admin {
                public String role;
            }
        ",
        "com/example/Test.java" => r"
            package com.example;

            import com.alpha.*;
            import com.beta.*;

            public class Test {
                public void test() {
                    Nonexistent<|>Class obj = null;
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// A parsed invocation with known, incompatible arguments must not fall back to
// the only declaration with that name.
lsp_goto_none_test! {
    name: inapplicable_unique_overload_is_not_a_navigation_target,
    files: {
        "com/example/Target.java" => r"
            package com.example;

            public class Target {
                public void accept(int value) {}
            }
        ",
        "com/example/Test.java" => r#"
            package com.example;

            public class Test {
                public void test(Target target) {
                    target.acc<|>ept("not an int");
                }
            }
        "#
    },
    nav: "com/example/Test.java", "<|>"
}

// Neither incomparable reference overload is more specific for null.
lsp_goto_none_test! {
    name: incomparable_null_overloads_remain_ambiguous,
    files: {
        "com/example/Target.java" => r"
            package com.example;

            public class Target {
                public void accept(String value) {}
                public void accept(Integer value) {}
            }
        ",
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test(Target target) {
                    target.acc<|>ept(null);
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Navigate on unused import symbol that doesn't exist - should return None
lsp_goto_none_test! {
    name: unused_import,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            import com.example.model.NonExistent<|>Type;

            public class Test {
                public void test() {}
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// ============================================================================
// Type System Edge Cases (4 tests)
// ============================================================================

// Method reference to undefined method - should return None
lsp_goto_none_test! {
    name: method_reference_undefined,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            import java.util.List;
            import java.util.ArrayList;

            public class Test {
                public void test() {
                    List<String> list = new ArrayList<>();
                    list.forEach(System.out::undefined<|>Method);
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Navigate on generic type variable T - should return None (type variable, not a class)
lsp_goto_none_test! {
    name: generic_type_variable,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {}
        ",
        "com/example/Container.java" => r"
            package com.example;

            public class Container<T<|> extends User> {
                private T value;

                public T getValue() {
                    return value;
                }
            }
        "
    },
    nav: "com/example/Container.java", "<|>"
}

// Navigate on unresolved superclass - should return None
lsp_goto_none_test! {
    name: unresolved_superclass,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            public class Test extends NonExistent<|>Base {
                public void test() {}
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Diamond operator with unresolved type - should return None
lsp_goto_none_test! {
    name: diamond_operator_unresolved,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            import java.util.List;
            import java.util.ArrayList;

            public class Test {
                public void test() {
                    List<NonExistent<|>Type> list = new ArrayList<>();
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// ============================================================================
// Parse Errors and Incomplete Code (3 tests)
// ============================================================================

// Navigate in incomplete expression - should return None
lsp_goto_none_test! {
    name: incomplete_expression,
    files: {
        "com/example/User.java" => r#"
            package com.example;

            public class User {
                public String getName() { return ""; }
            }
        "#,
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test() {
                    User user = new User();
                    user.<|>
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Navigate on undefined static field in nested expression - should return None
lsp_goto_none_test! {
    name: undefined_static_field,
    files: {
        "com/example/Config.java" => r#"
            package com.example;

            public class Config {
                public static String DEBUG = "debug";
            }
        "#,
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test() {
                    String value = Config.UNDEFINED<|>_CONSTANT;
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

// Navigate on field inside anonymous class with undefined reference - should return None
lsp_goto_none_test! {
    name: anonymous_class_field,
    files: {
        "com/example/Test.java" => r"
            package com.example;

            interface Runnable {
                void run();
            }

            public class Test {
                public void test() {
                    Runnable r = new Runnable() {
                        private int counter;

                        @Override
                        public void run() {
                            nonExistent<|>Field = 1;
                        }
                    };
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

#[test]
fn wildcard_capture_navigation_distinguishes_safe_and_invalid_writes() {
    use crate::lsp_test_framework::{goto_response_to_locations, LspTest};

    for (receiver, argument, applicable) in [
        ("? super Base", "new Base()", true),
        ("? super Base", "new Child()", true),
        ("? super Base", "new Object()", false),
        ("? extends Base", "new Base()", false),
        ("?", "new Base()", false),
        ("? extends Base", "null", true),
        ("?", "null", true),
    ] {
        let mut test = LspTest::new();
        test.add_file(
            "p/Sink.java",
            "package p; public interface Sink<E> { void put(E value); }",
        );
        test.add_file(
            "p/Use.java",
            &format!(
                "package p; class Base {{}} class Child extends Base {{}} \
             class Use {{ void run(Sink<{receiver}> sink) {{ sink.«call»put({argument}); }} }}"
            ),
        );
        let context = test.into_shared_context_with_options(false).unwrap();
        let locations =
            goto_response_to_locations(context.goto_definition("p/Use.java", "«call»").unwrap());
        assert_eq!(
            locations.len(),
            usize::from(applicable),
            "Sink<{receiver}>.put({argument}): {locations:?}"
        );
        if applicable {
            assert!(locations[0].uri.path().ends_with("p/Sink.java"));
        }
    }
}
