// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive Stream API and Lambda tests
// Testing: lambda expressions, method references, stream operations, type inference
lsp_test_suite! {
    name: "Stream API and Lambdas",

    // Basic lambda expression
    test basic_lambda {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.function.Function;
                public class Main {
                    public void test() {
                        Function<String, Integer> fn = s -> s.leng<|>th();
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
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    // Stream map operation
    test stream_map {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test() {
                        List<String> names = Arrays.asList("a", "b");
                        List<Integer> lengths = names.stream()
                            .map(s -> s.leng<|>th())
                            .collect(Collectors.toList());
                    }
                }
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public int length() { return 0; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    // Stream filter
    test stream_filter {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test(List<User> users) {
                        List<User> active = users.stream()
                            .filter(u -> u.isAct<|>ive())
                            .collect(Collectors.toList());
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    public boolean isActive() { return true; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Method reference regression: keep coverage for receiver-bound references (logger::println).
    test method_reference {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    private Logger logger = new Logger();

                    public void test(List<MyString> list) {
                        list.forEach(logger::print<|>ln);
                    }
                }
            ",
            "com/example/Logger.java" => r"
                package com.example;
                public class Logger {
                    public void println(MyString x) {}
                }
            ",
            "com/example/MyString.java" => r"
                package com.example;
                public class MyString {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "Logger.java"
    }

    // Method reference on instance
    test instance_method_reference {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test() {
                        List<String> list = Arrays.asList("a", "b");
                        list.stream().map(String::toUpperC<|>ase);
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
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    // Constructor reference
    test constructor_reference {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test() {
                        List<String> names = Arrays.asList("a", "b");
                        List<User> users = names.stream()
                            .map(Us<|>er::new)
                            .collect(Collectors.toList());
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    public User(String name) {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Chained stream operations
    test chained_stream_ops {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test(List<User> users) {
                        users.stream()
                            .filter(u -> u.isActive())
                            .map(u -> u.getNa<|>me())
                            .collect(Collectors.toList());
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;
                public class User {
                    public boolean isActive() { return true; }
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Optional map
    test optional_map {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.Optional;
                public class Main {
                    public void test(Optional<User> userOpt) {
                        String name = userOpt
                            .map(u -> u.getNa<|>me())
                            .orElse("Unknown");
                    }
                }
            "#,
            "com/example/User.java" => r#"
                package com.example;
                public class User {
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Stream flatMap
    test stream_flatmap {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test(List<LibraryRoom> rooms) {
                        List<User> allUsers = rooms.stream()
                            .flatMap(g -> g.getUs<|>ers().stream())
                            .collect(Collectors.toList());
                    }
                }
            ",
            "com/example/LibraryRoom.java" => r"
                package com.example;
                import java.util.*;
                public class LibraryRoom {
                    public List<User> getUsers() { return null; }
                }
            ",
            "com/example/User.java" => "package com.example; public class User {}"
        },
        nav: "com/example/Main.java", "<|>" => "LibraryRoom.java"
    }

    // Test: Navigate to stream() method on List - verifies generic parameter extraction
    test stream_method_on_list {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test() {
                        List<String> names = Arrays.asList("a", "b");
                        // Navigate to stream() - should resolve to List.stream()
                        names.str<|>eam().map(s -> s.length()).collect(Collectors.toList());
                    }
                }
            "#,
            "java/util/List.java" => r"
                package java.util;
                import java.util.stream.Stream;
                public interface List<E> extends Collection<E> {
                    Stream<E> stream();
                }
            ",
            "java/util/Collection.java" => "package java.util; public interface Collection<E> {}"
        },
        nav: "com/example/Main.java", "<|>" => "List.java"
    }
}
