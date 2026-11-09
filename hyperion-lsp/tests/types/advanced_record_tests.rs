// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for advanced record features

lsp_test_suite! {
    name: "Generic Records",

    test generic_record_basic {
        files: {
            "com/example/Pair.java" => r"
                package com.example;

                public record Pair<T, U>(T first, U second) {}
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Pair<String, Integer> pair = new Pa<|>ir<>("hello", 42);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Pair.java"
    }

    test generic_record_component_access {
        files: {
            "com/example/Pair.java" => r"
                package com.example;

                public record Pair<T, U>(T first, U second) {}
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Pair<String, Integer> pair = new Pair<>("hello", 42);
                        String value = pair.fir<|>st();
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Pair.java"
    }

    test generic_record_nested {
        files: {
            "com/example/Container.java" => r"
                package com.example;

                public record Container<T>(T value) {
                    public T getValue() {
                        return value;
                    }
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Container<String> container = new Container<>("test");
                        String val = container.get<|>Value();
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Container.java"
    }

    test generic_record_multiple_type_params {
        files: {
            "com/example/Triple.java" => r"
                package com.example;

                public record Triple<A, B, C>(A first, B second, C third) {}
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Triple<String, Integer, Boolean> triple =
                            new Trip<|>le<>("test", 42, true);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Triple.java"
    }
}

lsp_test_suite! {
    name: "Compact Constructors",

    test record_with_compact_constructor {
        files: {
            "com/example/Point.java" => r#"
                package com.example;

                public record Point(int x, int y) {
                    public Point {
                        if (x < 0 || y < 0) {
                            throw new IllegalArgumentException("Coordinates must be positive");
                        }
                    }
                }
            "#,
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Point point = new Po<|>int(10, 20);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Point.java"
    }

    test compact_constructor_with_component_access {
        files: {
            "com/example/Range.java" => r#"
                package com.example;

                public record Range(int start, int end) {
                    public Range {
                        if (start > end) {
                            throw new IllegalArgumentException("Start must be <= end");
                        }
                    }

                    public int size() {
                        return end - start;
                    }
                }
            "#,
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Range range = new Range(1, 10);
                        int s = range.si<|>ze();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Range.java"
    }
}

lsp_test_suite! {
    name: "Canonical Constructors",

    test record_with_canonical_constructor {
        files: {
            "com/example/Person.java" => r#"
                package com.example;

                public record Person(String name, int age) {
                    public Person(String name, int age) {
                        if (name == null || name.isEmpty()) {
                            throw new IllegalArgumentException("Name cannot be empty");
                        }
                        if (age < 0 || age > 150) {
                            throw new IllegalArgumentException("Invalid age");
                        }
                        this.name = name;
                        this.age = age;
                    }
                }
            "#,
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Person person = new Per<|>son("Alice", 30);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Person.java"
    }
}

lsp_test_suite! {
    name: "Records with Static Members",

    test record_with_static_field {
        files: {
            "com/example/Config.java" => r#"
                package com.example;

                public record Config(String value) {
                    public static final Config DEFAULT = new Config("default");

                    public static Config create(String value) {
                        return new Config(value);
                    }
                }
            "#,
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Config config = Config.DE<|>FAULT;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Config.java"
    }

    test record_static_method_navigation {
        files: {
            "com/example/Config.java" => r#"
                package com.example;

                public record Config(String value) {
                    public static final Config DEFAULT = new Config("default");

                    public static Config create(String value) {
                        return new Config(value);
                    }
                }
            "#,
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Config config = Config.cre<|>ate("test");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Config.java"
    }
}

lsp_test_suite! {
    name: "Records Implementing Interfaces",

    test record_implements_interface {
        files: {
            "com/example/Printable.java" => r"
                package com.example;

                public interface Printable {
                    void print();
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public record User(String name, int age) implements Printable {
                    @Override
                    public void print() {
                        System.out.println(name + " is " + age + " years old");
                    }
                }
            "#,
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new Us<|>er("Alice", 30);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test record_interface_method_navigation {
        files: {
            "com/example/Printable.java" => r"
                package com.example;

                public interface Printable {
                    void print();
                }
            ",
            "com/example/User.java" => r#"
                package com.example;

                public record User(String name, int age) implements Printable {
                    @Override
                    public void print() {
                        System.out.println(name + " is " + age + " years old");
                    }
                }
            "#,
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new User("Alice", 30);
                        user.pri<|>nt();
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test generic_record_with_interface {
        files: {
            "com/example/Comparable.java" => r"
                package com.example;

                public interface Comparable<T> {
                    int compareTo(T other);
                }
            ",
            "com/example/Pair.java" => r"
                package com.example;

                public record Pair<T extends Comparable<T>>(T first, T second)
                    implements Comparable<Pair<T>> {

                    @Override
                    public int compareTo(Pair<T> other) {
                        return this.first.compareTo(other.first);
                    }
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Pair<String> pair = new Pair<>("a", "b");
                        int result = pair.compare<|>To(new Pair<>("c", "d"));
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Pair.java"
    }
}
