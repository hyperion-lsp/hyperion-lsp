// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Test for Argument Type Inference on Complex Expressions
//
// This test verifies that argument type inference works on complex expressions
// (NAME_EXPR, FIELD_ACCESS_EXPR, METHOD_CALL_EXPR) instead of giving up with Type::Unknown.
//
// Strategy: Use goto-definition through method chains to verify type inference.
// If Arrays.asList(user.getName()) correctly infers List<String>, then calling
// .get(0) on it should navigate to List.java when hovering over the result.

lsp_test_suite! {
    name: "Complex Argument Type Inference",

    test variable_argument {
        files: {
            "java/util/Arrays.java" => r"
                package java.util;
                public class Arrays {
                    public static <T> List<T> asList(T... elements) {
                        return null;
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    E get(int index);
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public int length() { return 0; }
                }
            ",
            "com/example/Test.java" => r#"
                package com.example;
                import java.util.Arrays;

                public class Test {
                    public void test() {
                        String name = "Alice";
                        var list = Arrays.asList(name);
                        list.get(0).len<|>gth();
                    }
                }
            "#
        },
        nav: "com/example/Test.java", "<|>" => "String.java"
    }

    test field_access_argument {
        files: {
            "java/util/Arrays.java" => r"
                package java.util;
                public class Arrays {
                    public static <T> List<T> asList(T... elements) {
                        return null;
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    E get(int index);
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public int length() { return 0; }
                }
            ",
            "com/example/Test.java" => r#"
                package com.example;
                import java.util.Arrays;

                public class Test {
                    private String name = "Alice";

                    public void test() {
                        var list = Arrays.asList(this.name);
                        list.get(0).len<|>gth();
                    }
                }
            "#
        },
        nav: "com/example/Test.java", "<|>" => "String.java"
    }

    test method_call_argument {
        files: {
            "java/util/Arrays.java" => r"
                package java.util;
                public class Arrays {
                    public static <T> List<T> asList(T... elements) {
                        return null;
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    E get(int index);
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public int length() { return 0; }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() {
                        return "Alice";
                    }
                }
            "#,
            "com/example/Test.java" => r"
                package com.example;
                import java.util.Arrays;

                public class Test {
                    public void test(User user, User otherUser) {
                        var list = Arrays.asList(user.getName(), otherUser.getName());
                        list.get(0).len<|>gth();
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "String.java"
    }

    test nested_method_call_argument {
        files: {
            "java/util/Arrays.java" => r"
                package java.util;
                public class Arrays {
                    public static <T> List<T> asList(T... elements) {
                        return null;
                    }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    E get(int index);
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public int length() { return 0; }
                }
            ",
            "com/example/UserService.java" => r"
                package com.example;

                public class UserService {
                    public User getUser() {
                        return new User();
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public class User {
                    public String getName() {
                        return "Alice";
                    }
                }
            "#,
            "com/example/Test.java" => r"
                package com.example;
                import java.util.Arrays;

                public class Test {
                    public void test(UserService service) {
                        var list = Arrays.asList(service.getUser().getName());
                        list.get(0).len<|>gth();
                    }
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "String.java"
    }
}
