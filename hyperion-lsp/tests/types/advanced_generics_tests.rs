// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive generic type inference tests
// Testing: generic methods, generic classes, bounded types, wildcards
lsp_test_suite! {
    name: "Advanced Generic Type Inference",

    // Generic method call
    test generic_method_call {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test() {
                        String first = Collections.firstEleme<|>nt(Arrays.asList("a", "b"));
                    }
                }
            "#,
            "com/example/Collections.java" => r"
                package com.example;
                import java.util.List;
                public class Collections {
                    public static <T> T firstElement(List<T> list) {
                        return list.get(0);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Collections.java"
    }

    // Generic class instantiation
    test generic_class_instantiation {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Container<String> container = new Contain<|>er<>();
                    }
                }
            ",
            "com/example/Container.java" => r"
                package com.example;
                public class Container<T> {
                    private T value;
                    public T getValue() { return value; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Container.java"
    }

    // Generic method with bounded type
    test bounded_generic_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test() {
                        List<Integer> nums = Arrays.asList(1, 2, 3);
                        int max = Utils.findM<|>ax(nums);
                    }
                }
            ",
            "com/example/Utils.java" => r"
                package com.example;
                import java.util.List;
                public class Utils {
                    public static <T extends Comparable<T>> T findMax(List<T> list) {
                        return null;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    // List generic type
    test list_generic_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test() {
                        List<User> users = new ArrayList<>();
                        users.add(new Us<|>er());
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                public class User {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Map generic types
    test map_generic_types {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test() {
                        Map<String, User> userMap = new HashMap<>();
                        userMap.put("key", new Us<|>er());
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Generic field access
    test generic_field_access {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    private Container<User> userContainer;

                    public void test() {
                        User user = userContainer.getVal<|>ue();
                    }
                }
            ",
            "com/example/Container.java" => r"
                package com.example;
                public class Container<T> {
                    private T value;
                    public T getValue() { return value; }
                }
            ",
            "com/example/User.java" => "package com.example; public class User {}"
        },
        nav: "com/example/Main.java", "<|>" => "Container.java"
    }

    // Wildcard extends
    test wildcard_extends {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test(List<? extends Number> numbers) {
                        for (Number n : numbers) {
                            n.intVal<|>ue();
                        }
                    }
                }
            ",
            "java/lang/Number.java" => r"
                package java.lang;
                public class Number {
                    public int intValue() { return 0; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Number.java"
    }

    // Diamond operator
    test diamond_operator {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test() {
                        List<User> users = new ArrayLi<|>st<>();
                    }
                }
            ",
            "java/util/ArrayList.java" => r"
                package java.util;
                public class ArrayList<E> extends AbstractList<E> implements List<E> {
                    public ArrayList() {}
                }
            ",
            "java/util/List.java" => "package java.util; public interface List<E> {}",
            "java/util/AbstractList.java" => "package java.util; public abstract class AbstractList<E> {}",
            "com/example/User.java" => "package com.example; public class User {}"
        },
        nav: "com/example/Main.java", "<|>" => "ArrayList.java"
    }
}
