// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for var type inference (Java 10+)
// Priority 1: High priority - very common in modern codebases

use serial_test::serial;

lsp_test_suite! {
    name: "Var Type Inference Navigation",

    test var_inferred_from_constructor {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        var user = new User();
                        user.get<|>Name();
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

    test var_from_method_return {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(UserService service) {
                        var user = service.getUser();
                        user.get<|>Name();
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;
                public class User {
                    public String getName() { return ""; }
                }
            "#,
            "com/example/UserService.java" => r"
                package com.example;
                public class UserService {
                    public User getUser() { return new User(); }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test var_with_generic_type {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;

                public class Main {
                    public void test() {
                        var users = new ArrayList<User>();
                        users.ad<|>d(new User());
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                public class User {}
            ",
            "java/util/ArrayList.java" => r"
                package java.util;
                public class ArrayList<E> implements List<E> {
                    public boolean add(E e) { return true; }
                }
            ",
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    boolean add(E e);
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "ArrayList.java"
    }

    test var_chained_method_calls {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        var service = new UserService();
                        var user = service.getUser();
                        var name = user.getName();
                        name.toUpperCa<|>se();
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;
                public class User {
                    public String getName() { return ""; }
                }
            "#,
            "com/example/UserService.java" => r"
                package com.example;
                public class UserService {
                    public User getUser() { return new User(); }
                }
            ",
            "java/lang/String.java" => r"
                package java.lang;
                public final class String {
                    public String toUpperCase() { return this; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }


    test var_in_for_loop {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;

                public class Main {
                    public void test(List<User> users) {
                        for (var user : users) {
                            user.get<|>Name();
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

    test var_from_ternary_operator {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(boolean condition) {
                        var user = condition ? new User("John") : new User("Jane");
                        user.get<|>Name();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;
                    public User(String name) { this.name = name; }
                    public String getName() { return name; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test var_with_diamond_operator {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.*;

                public class Main {
                    public void test() {
                        var map = new HashMap<String, User>();
                        map.pu<|>t("key", new User());
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {}
            ",
            "java/util/HashMap.java" => r"
                package java.util;
                public class HashMap<K, V> implements Map<K, V> {
                    public V put(K key, V value) { return null; }
                }
            ",
            "java/util/Map.java" => r"
                package java.util;
                public interface Map<K, V> {
                    V put(K key, V value);
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "HashMap.java"
    }

    test var_from_lambda_result {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.function.*;

                public class Main {
                    public void test() {
                        Supplier<User> supplier = () -> new User();
                        var user = supplier.get();
                        user.get<|>Name();
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;
                public class User {
                    public String getName() { return ""; }
                }
            "#,
            "java/util/function/Supplier.java" => r"
                package java.util.function;
                public interface Supplier<T> {
                    T get();
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test var_with_builder_pattern {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        var user = User.builder()
                            .name("John")
                            .age(30)
                            .build();
                        user.get<|>Name();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;
                    private int age;

                    public static Builder builder() { return new Builder(); }
                    public String getName() { return name; }

                    public static class Builder {
                        private User user = new User();
                        public Builder name(String name) {
                            user.name = name;
                            return this;
                        }
                        public Builder age(int age) {
                            user.age = age;
                            return this;
                        }
                        public User build() { return user; }
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }



    test var_from_array_access {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(User[] users) {
                        var user = users[0];
                        user.get<|>Name();
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

    test var_with_cast {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(Object obj) {
                        var user = (User) obj;
                        user.get<|>Name();
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

    test var_in_try_with_resources {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.io.*;

                public class Main {
                    public void test() {
                        try (var reader = new BufferedReader(new FileReader("file.txt"))) {
                            reader.read<|>Line();
                        }
                    }
                }
            "#,
            "java/io/BufferedReader.java" => r#"
                package java.io;
                public class BufferedReader extends Reader {
                    public BufferedReader(Reader in) {}
                    public String readLine() { return ""; }
                }
            "#,
            "java/io/Reader.java" => r"
                package java.io;
                public abstract class Reader {
                    public void close() {}
                }
            ",
            "java/io/FileReader.java" => r"
                package java.io;
                public class FileReader extends Reader {
                    public FileReader(String fileName) {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "BufferedReader.java"
    }

}

// ============================================================================
// LUB (Least Upper Bound) Tests - Ternary Expression Type Inference
// ============================================================================

lsp_test_suite! {
    name: "Ternary Expression LUB",

    test ternary_same_type {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(boolean condition) {
                        var result = condition ? new User("John") : new User("Jane");
                        result.get<|>Name();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;
                    public User(String name) { this.name = name; }
                    public String getName() { return name; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test ternary_subtype_relationship {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(boolean condition) {
                        // Admin extends User, so Admin and User -> User
                        var result = condition ? new Admin("John") : new User("Jane");
                        result.get<|>Name();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;
                    public User(String name) { this.name = name; }
                    public String getName() { return name; }
                }
            ",
            "com/example/Admin.java" => r"
                package com.example;
                public class Admin extends User {
                    public Admin(String name) { super(name); }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test ternary_with_null {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(boolean condition) {
                        // User and null -> User
                        var result = condition ? new User("John") : null;
                        result.get<|>Name();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;
                    public User(String name) { this.name = name; }
                    public String getName() { return name; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test ternary_interface_implementation {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(boolean condition) {
                        // Both implement Readable, so LUB should be Readable
                        var result = condition ? new Book() : new Article();
                        result.re<|>ad();
                    }
                }
            ",
            "com/example/Readable.java" => r"
                package com.example;
                public interface Readable {
                    void read();
                }
            ",
            "com/example/Book.java" => r"
                package com.example;
                public class Book implements Readable {
                    public void read() {}
                }
            ",
            "com/example/Article.java" => r"
                package com.example;
                public class Article implements Readable {
                    public void read() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Readable.java"
    }

    test ternary_nested {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(boolean c1, boolean c2) {
                        // Nested ternary: User ? User : User -> User
                        var result = c1 ? new User("A") : (c2 ? new User("B") : new User("C"));
                        result.get<|>Name();
                    }
                }
            "#,
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    private String name;
                    public User(String name) { this.name = name; }
                    public String getName() { return name; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

// ============================================================================
// ADVANCED TESTS - Type system features under development
// ============================================================================

#[test]
fn var_from_static_method() {
    let mut test = crate::lsp_test_framework::LspTest::new();

    test.add_file(
        "com/example/Main.java",
        r#"
        package com.example;
        import java.util.*;

        public class Main {
            public void test() {
                var list = Arrays.asList("a", "b", "c");
                list.siz<|>e();
            }
        }
    "#,
    );
    test.add_file(
        "java/util/Arrays.java",
        r"
        package java.util;
        public class Arrays {
            public static <T> List<T> asList(T... elements) {
                return null;
            }
        }
    ",
    );
    test.add_file(
        "java/util/List.java",
        r"
        package java.util;
        public interface List<E> {
            int size();
        }
    ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");
    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(loc.uri.as_str().contains("List.java"));
        }
        _ => panic!("Expected definition in List.java, got None"),
    }
}

#[test]
#[serial]
fn var_from_multi_letter_method_type_parameter() {
    let mut test = crate::lsp_test_framework::LspTest::new();

    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;

        public class Main {
            public void test() {
                var user = Factory.identity(new User());
                user.get<|>Name();
            }
        }
    ",
    );
    test.add_file(
        "com/example/Factory.java",
        r"
        package com.example;

        public class Factory {
            public static <Result> Result identity(Result value) {
                return value;
            }
        }
    ",
    );
    test.add_file(
        "com/example/User.java",
        r#"
        package com.example;

        public class User {
            public String getName() { return ""; }
        }
    "#,
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");
    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains("User.java"),
                "Expected navigation through declared method type parameter, got: {}",
                loc.uri
            );
        }
        Ok(other) => panic!("Expected definition in User.java, got {other:?}"),
        Err(error) => panic!("Expected definition in User.java, got error: {error:?}"),
    }
}

#[test]
fn var_from_optional() {
    let mut test = crate::lsp_test_framework::LspTest::new();

    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;
        import java.util.*;

        public class Main {
            public void test() {
                var optional = Optional.of(new User());
                var user = optional.get();
                user.get<|>Name();
            }
        }
    ",
    );
    test.add_file(
        "com/example/User.java",
        r#"
        package com.example;
        public class User {
            public String getName() { return ""; }
        }
    "#,
    );
    test.add_file(
        "java/util/Optional.java",
        r"
        package java.util;
        public class Optional<T> {
            public static <T> Optional<T> of(T value) { return null; }
            public T get() { return null; }
        }
    ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");
    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(loc.uri.as_str().contains("User.java"));
        }
        _ => panic!("Expected definition in User.java, got None"),
    }
}

// `var` inferred through Stream.map(User::getName).collect(...).
#[test]
fn var_from_stream_collect() {
    let mut test = crate::lsp_test_framework::LspTest::new();

    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;
        import java.util.*;
        import java.util.stream.*;

        public class Main {
            public void test(List<User> users) {
                var names = users.stream()
                    .map(User::getName)
                    .collect(Collectors.toList());
                names.siz<|>e();
            }
        }
    ",
    );
    test.add_file(
        "com/example/QualifiedMain.java",
        r"
        package com.example;
        import java.util.*;

        public class QualifiedMain {
            public void test(List<User> users) {
                var names = users.stream()
                    .map(User::getName)
                    .collect(java.util.stream.Collectors.toList());
                names.siz<|>e();
            }
        }
    ",
    );
    test.add_file(
        "com/example/StaticImportMain.java",
        r"
        package com.example;
        import java.util.*;
        import static java.util.stream.Collectors.toList;

        public class StaticImportMain {
            public void test(List<User> users) {
                var names = users.stream()
                    .map(User::getName)
                    .collect(toList());
                names.siz<|>e();
            }
        }
    ",
    );
    test.add_file(
        "com/example/User.java",
        r#"
        package com.example;
        public class User {
            public String getName() { return ""; }
        }
    "#,
    );
    test.add_file(
        "java/util/List.java",
        r"
        package java.util;
        public interface List<E> extends Collection<E> {
            int size();
            java.util.stream.Stream<E> stream();
        }
    ",
    );
    test.add_file(
        "java/util/Collection.java",
        r"
        package java.util;
        public interface Collection<E> {
            java.util.stream.Stream<E> stream();
        }
    ",
    );
    test.add_file(
        "java/util/stream/Stream.java",
        r"
        package java.util.stream;
        public interface Stream<T> {
            <R> Stream<R> map(java.util.function.Function<T, R> mapper);
            Stream<T> filter(java.util.function.Predicate<T> predicate);
            <R> R collect(Collector<T, ?, R> collector);
        }
    ",
    );
    test.add_file(
        "java/util/stream/Collector.java",
        r"
        package java.util.stream;
        public interface Collector<T, A, R> {
        }
    ",
    );
    test.add_file(
        "java/util/stream/Collectors.java",
        r"
        package java.util.stream;
        public class Collectors {
            public static <T> Collector<T, ?, java.util.List<T>> toList() { return null; }
            public static <T> Collector<T, ?, java.util.Set<T>> toSet() { return null; }
        }
    ",
    );
    test.add_file(
        "java/util/function/Function.java",
        r"
        package java.util.function;
        public interface Function<T, R> {
            R apply(T t);
        }
    ",
    );
    test.add_file(
        "java/util/function/Predicate.java",
        r"
        package java.util.function;
        public interface Predicate<T> {
            boolean test(T t);
        }
    ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");
    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(loc.uri.as_str().contains("List.java"));
        }
        _ => panic!("Expected definition in List.java, got None"),
    }

    let qualified_result = test.goto_definition("com/example/QualifiedMain.java", "<|>");
    match qualified_result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(loc.uri.as_str().contains("List.java"));
        }
        _ => panic!("Expected qualified collector result in List.java, got None"),
    }

    let static_import_result = test.goto_definition("com/example/StaticImportMain.java", "<|>");
    match static_import_result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(loc.uri.as_str().contains("List.java"));
        }
        _ => panic!("Expected static-import collector result in List.java, got None"),
    }
}

// Members of an intersection-bounded type parameter (T extends A & B) resolve.
#[test]
fn var_intersection_type() {
    let mut test = crate::lsp_test_framework::LspTest::new();

    test.add_file(
        "com/example/Main.java",
        r"
        package com.example;
        import java.io.*;

        public class Main {
            public <T extends Closeable & Serializable> void test(T resource) {
                var res = resource;
                res.clo<|>se();
            }
        }
    ",
    );
    test.add_file(
        "java/io/Closeable.java",
        r"
        package java.io;
        public interface Closeable {
            void close();
        }
    ",
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");
    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            assert!(loc.uri.as_str().contains("Closeable.java"));
        }
        _ => panic!("Expected definition in Closeable.java, got None"),
    }
}

/// Test nested generics: Map<String, List<User>>.get() should return List<User>
/// This validates recursive type parameter substitution
/// Note: This test is marked serial due to race conditions with JDK class cache
#[test]
#[serial]
fn var_from_nested_generics() {
    let mut test = crate::lsp_test_framework::LspTest::new();

    // Add stub for java.util.List
    test.add_file(
        "java/util/List.java",
        r"
        package java.util;

        public interface List<E> {
            E get(int index);
            boolean add(E element);
        }
    ",
    );

    // Add stub for java.util.ArrayList
    test.add_file(
        "java/util/ArrayList.java",
        r"
        package java.util;

        public class ArrayList<E> implements List<E> {
            public E get(int index) { return null; }
            public boolean add(E element) { return true; }
        }
    ",
    );

    // Add stub for java.util.Map
    test.add_file(
        "java/util/Map.java",
        r"
        package java.util;

        public interface Map<K, V> {
            V get(Object key);
        }
    ",
    );

    // Add stub for java.util.HashMap
    test.add_file(
        "java/util/HashMap.java",
        r"
        package java.util;

        public class HashMap<K, V> implements Map<K, V> {
            public V get(Object key) { return null; }
        }
    ",
    );

    test.add_file(
        "com/example/User.java",
        r#"
        package com.example;

        public class User {
            public String getName() { return ""; }
        }
    "#,
    );

    test.add_file(
        "com/example/Main.java",
        r#"
        package com.example;
        import java.util.*;

        public class Main {
            public void test() {
                var userMap = new HashMap<String, List<User>>();
                var users = userMap.get("key");
                var firstUser = users.get(0);
                firstUser.get<|>Name();
            }
        }
    "#,
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");
    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            // Should navigate to User.java because:
            // 1. userMap.get() returns List<User> (V from Map<String, List<User>>)
            // 2. users.get(0) returns User (E from List<User>)
            // 3. firstUser.getName() navigates to User.getName
            assert!(
                loc.uri.as_str().contains("User.java"),
                "Expected navigation to User.java (nested generic extraction), got: {}",
                loc.uri
            );
        }
        Ok(other) => panic!("Expected definition in User.java, got {:?}", other),
        Err(e) => panic!("Expected definition in User.java, got error: {:?}", e),
    }
}

/// Test that Map<K,V>.get() returns V (value type), not K (key type)
/// This validates proper type parameter position mapping
/// Note: This test is marked serial due to race conditions with JDK class cache
#[test]
#[serial]
fn var_from_map_get() {
    let mut test = crate::lsp_test_framework::LspTest::new();

    // Add stub for java.util.Map with proper type parameters
    test.add_file(
        "java/util/Map.java",
        r"
        package java.util;

        public interface Map<K, V> {
            V get(Object key);
            V put(K key, V value);
        }
    ",
    );

    // Add stub for java.util.HashMap
    test.add_file(
        "java/util/HashMap.java",
        r"
        package java.util;

        public class HashMap<K, V> implements Map<K, V> {
            public V get(Object key) { return null; }
            public V put(K key, V value) { return null; }
        }
    ",
    );

    test.add_file(
        "com/example/User.java",
        r#"
        package com.example;

        public class User {
            public String getName() { return ""; }
        }
    "#,
    );

    test.add_file(
        "com/example/Main.java",
        r#"
        package com.example;
        import java.util.*;

        public class Main {
            public void test() {
                var userMap = new HashMap<String, User>();
                var user = userMap.get("key");
                user.get<|>Name();
            }
        }
    "#,
    );

    let result = test.goto_definition("com/example/Main.java", "<|>");
    match result {
        Ok(Some(tower_lsp::lsp_types::GotoDefinitionResponse::Scalar(loc))) => {
            // Should navigate to User.java because user is of type User (V), not String (K)
            assert!(
                loc.uri.as_str().contains("User.java"),
                "Expected navigation to User.java (Map value type), got: {}",
                loc.uri
            );
        }
        Ok(other) => panic!("Expected definition in User.java, got {:?}", other),
        Err(e) => panic!("Expected definition in User.java, got error: {:?}", e),
    }
}
