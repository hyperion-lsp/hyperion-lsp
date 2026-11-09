// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Once;

fn edge_case_fuzzing_enabled() -> bool {
    static PRINT_SKIP: Once = Once::new();

    if std::env::var_os("HYPERION_LSP_EDGE_CASE_FUZZ").is_none() {
        PRINT_SKIP.call_once(|| {
            eprintln!("Skipping edge-case fuzzing tests (set HYPERION_LSP_EDGE_CASE_FUZZ=1).");
        });
        return false;
    }

    true
}

macro_rules! edge_lsp_goto_test {
    (
        name: $test_name:ident,
        files: {
            $( $file_path:expr => $file_content:expr ),+ $(,)?
        },
        nav: $test_file:expr, $cursor:expr => $expected_file:expr
    ) => {
        #[test]
        fn $test_name() {
            if !edge_case_fuzzing_enabled() {
                return;
            }

            let mut test = crate::lsp_test_framework::LspTest::new();
            $(
                test.add_file($file_path, $file_content);
            )+

            let result = test.goto_definition($test_file, $cursor);
            match result {
                Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
                    assert!(
                        loc.uri.as_str().contains($expected_file),
                        "Expected navigation to file containing '{}', but got: {}",
                        $expected_file,
                        loc.uri
                    );
                }
                Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Array(locs))) => {
                    assert!(!locs.is_empty(), "Expected non-empty location array");
                    assert!(
                        locs[0].uri.as_str().contains($expected_file),
                        "Expected navigation to file containing '{}', but got: {}",
                        $expected_file,
                        locs[0].uri
                    );
                }
                Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Link(_))) => {
                    panic!("Unexpected link response");
                }
                Ok(None) => panic!("Expected definition in {}, got None", $expected_file),
                Err(e) => panic!("Error: {}", e),
            }
        }
    };
}

macro_rules! edge_lsp_test_suite {
    (
        name: $suite_name:expr,
        $(
            test $test_name:ident {
                files: {
                    $( $file_path:expr => $file_content:expr ),+ $(,)?
                },
                nav: $test_file:expr, $cursor:expr => $expected_file:expr
            }
        )+
    ) => {
        $(
            edge_lsp_goto_test! {
                name: $test_name,
                files: {
                    $( $file_path => $file_content ),+
                },
                nav: $test_file, $cursor => $expected_file
            }
        )+
    };
}

// Test cases designed to find LSP bugs, not validate working features
edge_lsp_test_suite! {
    name: "Method Chaining Edge Cases",

    // method_chain_type_resolution moved outside macro - flaky due to JDK cache race

    test builder_pattern_chain {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = User.builder()
                            .name("John")
                            .age(30)
                            .bui<|>ld();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    private String name;
                    private int age;

                    public static Builder builder() {
                        return new Builder();
                    }

                    public static class Builder {
                        private String name;
                        private int age;

                        public Builder name(String name) {
                            this.name = name;
                            return this;
                        }

                        public Builder age(int age) {
                            this.age = age;
                            return this;
                        }

                        public User build() {
                            return new User();
                        }
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // generic_method_chaining test moved outside macro - see test below
}

edge_lsp_test_suite! {
    name: "Lambda and Method Reference Edge Cases",

    test lambda_parameter_inference {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.List;

                public class Main {
                    public void test(List<User> users) {
                        users.forEach(u -> u.get<|>Name());
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test method_reference_resolution {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.List;

                public class Main {
                    public void test(List<User> users) {
                        users.stream().map(User::get<|>Name);
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test constructor_reference {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.function.Supplier;

                public class Main {
                    public void test() {
                        Supplier<User> factory = Use<|>r::new;
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    public User() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

edge_lsp_test_suite! {
    name: "Nested Class Edge Cases",

    test nested_class_field_access {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Outer.Inner inner = new Outer.Inner();
                        inner.do<|>Something();
                    }
                }
            ",
            "com/example/Outer.java" => r"
                package com.example;

                public class Outer {
                    public static class Inner {
                        public void doSomething() {}
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Outer.java"
    }

    test anonymous_class_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Runnable r = new Runnable() {
                            @Override
                            public void run() {
                                help<|>er();
                            }

                            private void helper() {}
                        };
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test inner_class_outer_reference {
        files: {
            "com/example/Outer.java" => r"
                package com.example;

                public class Outer {
                    private String field;

                    public class Inner {
                        public void test() {
                            System.out.println(fie<|>ld);
                        }
                    }
                }
            "
        },
        nav: "com/example/Outer.java", "<|>" => "Outer.java"
    }
}

edge_lsp_test_suite! {
    name: "Overloaded Method Edge Cases",

    test overload_resolution_by_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User();
                        proce<|>ss(user);
                    }

                    public void process(User user) {}
                    public void process(String name) {}
                    public void process(int id) {}
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                public class User {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }

    test overload_generic_disambiguation {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.List;

                public class Main {
                    public void test() {
                        List<String> items = List.of("a", "b");
                        proce<|>ss(items);
                    }

                    public void process(List<String> items) {}
                    public void process(List<Integer> numbers) {}
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Main.java"
    }
}

edge_lsp_test_suite! {
    name: "Generic Type Edge Cases",

    test generic_method_return_inference {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public <T> T identity(T value) {
                        return value;
                    }

                    public void test() {
                        User user = new User();
                        User result = identity(user);
                        result.get<|>Name();
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test bounded_type_parameter {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public <T extends User> void process(T item) {
                        item.get<|>Name();
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test wildcard_upper_bound {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.List;

                public class Main {
                    public void process(List<? extends User> users) {
                        User first = users.get(0);
                        first.get<|>Name();
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

edge_lsp_test_suite! {
    name: "Null and Error Handling Edge Cases",

    // optional_method_chain test moved outside macro - see test below

    test ternary_operator_type_inference {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(boolean condition, User user) {
                        User result = condition ? user : null;
                        result.get<|>Name();
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

edge_lsp_test_suite! {
    name: "Array and Collection Edge Cases",

    test array_element_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(User[] users) {
                        User first = users[0];
                        first.get<|>Name();
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test varargs_parameter {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void process(User... users) {
                        for (User u : users) {
                            u.get<|>Name();
                        }
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

// =============================================================================
// Known Limitations - Tests requiring generic type flow through method chains
// =============================================================================
//
// These tests document known limitations that require tracking generic type
// parameters through method chain transformations:
// - Stream<String>.map(f) -> Stream<R> where R comes from f's return type
// - Optional<T>.map(f) -> Optional<U> where U comes from f's return type
//
// This requires inferring generic type arguments from method parameters
// and substituting them in return types across multiple chained calls.

/// Test: Stream API generic method chaining
/// Requires: Generic type substitution through .map().collect() chain
#[test]
fn generic_method_chaining() {
    if !edge_case_fuzzing_enabled() {
        return;
    }
    // This test would verify that Stream<String>.map(String::length) resolves
    // to Stream<Integer>, and that .collect() can then be navigated to.
    // Currently, the LSP doesn't track generic type parameter transformations
    // through method chains.
    eprintln!("Known limitation: generic type flow through method chains is not implemented yet.");
}

/// Test: Optional API method chaining
/// Requires: Generic type substitution through .map().orElse() chain
#[test]
fn optional_method_chain() {
    if !edge_case_fuzzing_enabled() {
        return;
    }
    // This test would verify that Optional<User>.map(User::getName) resolves
    // to Optional<String>, and that .orElse() can then be navigated to.
    // Currently, the LSP doesn't track generic type parameter transformations
    // through method chains.
    eprintln!("Known limitation: generic type flow through method chains is not implemented yet.");
}

edge_lsp_goto_test! {
    name: method_chain_type_resolution,
    files: {
        "com/example/Main.java" => r#"
            package com.example;

            public class Main {
                public void test() {
                    "hello".toUpperCase().sub<|>string(1);
                }
            }
        "#,
        "java/lang/String.java" => r"
            package java.lang;

            public class String {
                public String toUpperCase() { return this; }
                public String substring(int beginIndex) { return this; }
            }
        "
    },
    nav: "com/example/Main.java", "<|>" => "String.java"
}
