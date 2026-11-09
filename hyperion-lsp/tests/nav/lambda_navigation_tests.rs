// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Lambda parameter navigation tests
// Tests that goto-definition on lambda parameter usages navigates correctly

lsp_test_suite! {
    name: "Lambda Parameter Navigation",

    test consumer_lambda_method_call {
        files: {
            "test/Main.java" => r"
                package test;
                import java.util.function.Consumer;

                public class Main {
                    public void test() {
                        Consumer<String> c = s -> s.toUpper<|>Case();
                    }
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "test/Main.java", "<|>" => "String.java"
    }

    test function_lambda_method_call {
        files: {
            "test/Main.java" => r"
                package test;
                import java.util.function.Function;

                public class Main {
                    public void test() {
                        Function<String, Integer> f = s -> s.len<|>gth();
                    }
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public int length() { return 0; }
                }
            "
        },
        nav: "test/Main.java", "<|>" => "String.java"
    }

    test bifunction_first_param_method_call {
        files: {
            "test/Main.java" => r"
                package test;
                import java.util.function.BiFunction;

                public class Main {
                    public void test() {
                        BiFunction<String, Integer, Boolean> f =
                            (s, i) -> s.len<|>gth() > i;
                    }
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public int length() { return 0; }
                }
            "
        },
        nav: "test/Main.java", "<|>" => "String.java"
    }

    test stream_map_lambda_method_call {
        files: {
            "test/Main.java" => r#"
                package test;
                import java.util.List;

                public class Main {
                    public void test() {
                        List<String> names = List.of("Alice", "Bob");
                        names.stream().map(s -> s.toUpper<|>Case()).toList();
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "test/Main.java", "<|>" => "String.java"
    }

    test stream_filter_lambda_method_call {
        files: {
            "test/Main.java" => r#"
                package test;
                import java.util.List;

                public class Main {
                    public void test() {
                        List<String> names = List.of("Alice", "Bob");
                        names.stream().filter(s -> s.starts<|>With("A")).toList();
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public boolean startsWith(String prefix) { return false; }
                }
            "
        },
        nav: "test/Main.java", "<|>" => "String.java"
    }

    test user_class_in_lambda {
        files: {
            "test/Main.java" => r"
                package test;
                import java.util.function.Consumer;
                import test.model.User;

                public class Main {
                    public void test() {
                        Consumer<User> processor = user -> user.get<|>Name();
                    }
                }
            ",
            "test/model/User.java" => r#"
                package test.model;

                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "test/Main.java", "<|>" => "User.java"
    }

    test nested_lambda_outer_param {
        files: {
            "test/Main.java" => r"
                package test;
                import java.util.function.Function;
                import java.util.function.Consumer;

                public class Main {
                    public void test() {
                        Function<String, Consumer<String>> factory =
                            prefix -> msg -> System.out.println(prefix.toUpper<|>Case());
                    }
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "test/Main.java", "<|>" => "String.java"
    }

    test nested_lambda_inner_param {
        files: {
            "test/Main.java" => r"
                package test;
                import java.util.function.Function;
                import java.util.function.Consumer;

                public class Main {
                    public void test() {
                        Function<String, Consumer<String>> factory =
                            prefix -> msg -> System.out.println(msg.toUpper<|>Case());
                    }
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "test/Main.java", "<|>" => "String.java"
    }
}
