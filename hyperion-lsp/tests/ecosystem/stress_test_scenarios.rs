// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Stress tests to find the actual breaking points

lsp_test_suite! {
    name: "Stress Test Scenarios",

    // Test 1: Very deep method chaining
    test deep_method_chain {
        files: {
            "Test.java" => r#"
                package test;

                class A {
                    B getB() { return new B(); }
                }

                class B {
                    C getC() { return new C(); }
                }

                class C {
                    D getD() { return new D(); }
                }

                class D {
                    String getValue() { return ""; }
                }

                class Test {
                    void test() {
                        A a = new A();
                        String value = a.getB().getC().getD().getVa<|>lue();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 2: Stream API method chaining
    test stream_api_chain {
        files: {
            "Test.java" => r"
                package test;

                import java.util.List;
                import java.util.stream.Stream;

                class Test {
                    void test(List<String> list) {
                        long count = list.stream().filt<|>er(s -> s.length() > 0).count();
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                import java.util.stream.Stream;

                public interface List<E> {
                    Stream<E> stream();
                }
            ",
            "java/util/stream/Stream.java" => r"
                package java.util.stream;

                import java.util.function.Predicate;

                public interface Stream<T> {
                    Stream<T> filter(Predicate<? super T> predicate);
                    long count();
                }
            "
        },
        nav: "Test.java", "<|>" => "Stream.java"
    }

    // Test 3: Optional chaining
    test optional_chaining {
        files: {
            "Test.java" => r#"
                package test;

                import java.util.Optional;

                class Test {
                    void test() {
                        Optional<String> opt = Optional.of("test");
                        String value = opt.orEl<|>se("default");
                    }
                }
            "#,
            "java/util/Optional.java" => r"
                package java.util;

                public class Optional<T> {
                    public static <T> Optional<T> of(T value) { return null; }
                    public T orElse(T other) { return other; }
                }
            "
        },
        nav: "Test.java", "<|>" => "Optional.java"
    }

    // Test 4: Varargs method call
    test varargs_method_navigation {
        files: {
            "Test.java" => r#"
                package test;

                class Utils {
                    static String format(String pattern, Object... args) {
                        return "";
                    }
                }

                class Test {
                    void test() {
                        String result = Utils.form<|>at("Hello %s", "world");
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 5: Ternary operator type inference
    test ternary_operator_type {
        files: {
            "Test.java" => r#"
                package test;

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    void test(boolean condition) {
                        User user = condition ? new User() : new User();
                        String name = user.getN<|>ame();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 6: Cast expression
    test cast_expression_type {
        files: {
            "Test.java" => r#"
                package test;

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    void test(Object obj) {
                        User user = (User) obj;
                        String name = user.getN<|>ame();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 7: Array access type inference
    test array_element_type {
        files: {
            "Test.java" => r#"
                package test;

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    void test(User[] users) {
                        User user = users[0];
                        String name = user.getN<|>ame();
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 8: Enhanced for loop type inference
    test enhanced_for_loop_type {
        files: {
            "Test.java" => r#"
                package test;

                import java.util.List;

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    void test(List<User> users) {
                        for (User user : users) {
                            String name = user.getN<|>ame();
                        }
                    }
                }
            "#,
            "java/util/List.java" => r"
                package java.util;

                public interface List<E> extends Iterable<E> {
                }
            ",
            "java/lang/Iterable.java" => r"
                package java.lang;

                public interface Iterable<T> {
                }
            "
        },
        nav: "Test.java", "<|>" => "Test.java"
    }

    // Test 9: Static factory method return type
    test static_factory_return {
        files: {
            "Test.java" => r#"
                package test;

                import java.util.List;
                import java.util.Arrays;

                class Test {
                    void test() {
                        List<String> list = Arrays.asList("a", "b");
                        int size = list.si<|>ze();
                    }
                }
            "#,
            "java/util/Arrays.java" => r"
                package java.util;

                public class Arrays {
                    public static <T> List<T> asList(T... items) {
                        return null;
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;

                public interface List<E> {
                    int size();
                }
            "
        },
        nav: "Test.java", "<|>" => "List.java"
    }

    // Test 10: Instance of pattern variable (Java 16+)
    test instanceof_pattern_variable {
        files: {
            "Test.java" => r#"
                package test;

                class User {
                    String getName() { return ""; }
                }

                class Test {
                    void test(Object obj) {
                        if (obj instanceof User user) {
                            String name = user.getN<|>ame();
                        }
                    }
                }
            "#
        },
        nav: "Test.java", "<|>" => "Test.java"
    }
}

// Test for actual edge cases that might fail

lsp_goto_none_test! {
    name: reflection_method_call,
    files: {
        "Test.java" => r#"
            package test;

            import java.lang.reflect.Method;

            class Test {
                void test() throws Exception {
                    Method method = Test.class.getMethod("test");
                    Object result = method.inv<|>oke(this);
                }
            }
        "#
    },
    nav: "Test.java", "<|>"
}
