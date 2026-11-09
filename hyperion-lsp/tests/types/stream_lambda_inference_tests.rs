// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Stream API Lambda Type Inference Tests
//!
//! Tests for lambda parameter type inference in Stream API operations.
//! The type system must infer lambda parameter types from the stream's
//! element type and the functional interface.

// Test 1: map() with simple lambda
lsp_hover_test! {
    name: stream_map_lambda_parameter,

    files: {
        "Test.java" => r#"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<String> names = List.of("Alice", "Bob");
                    List<Integer> lengths = names.stream()
                        .map(n<|>ame -> name.length())
                        .collect(Collectors.toList());
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["String"]
}

// Test 2: filter() with predicate lambda
lsp_hover_test! {
    name: stream_filter_lambda_parameter,

    files: {
        "Test.java" => r"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<Integer> numbers = List.of(1, 2, 3, 4, 5);
                    List<Integer> evens = numbers.stream()
                        .filter(<|>n -> n % 2 == 0)
                        .collect(Collectors.toList());
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["Integer"]
}

// Test 3: flatMap() with nested streams
lsp_hover_test! {
    name: stream_flatmap_lambda_parameter,

    files: {
        "Test.java" => r#"
            import java.util.List;
            import java.util.stream.Stream;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<List<String>> nested = List.of(
                        List.of("a", "b"),
                        List.of("c", "d")
                    );
                    List<String> flat = nested.stream()
                        .flatMap(lis<|>t -> list.stream())
                        .collect(Collectors.toList());
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["List"]
}

// Test 4: forEach() terminal operation
lsp_hover_test! {
    name: stream_foreach_lambda_parameter,

    files: {
        "Test.java" => r#"
            import java.util.List;

            class Test {
                void test() {
                    List<String> names = List.of("Alice", "Bob");
                    names.stream()
                        .forEach(nam<|>e -> System.out.println(name));
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["String"]
}

// Test 5: reduce() with accumulator
lsp_hover_test! {
    name: stream_reduce_lambda_parameters,

    files: {
        "Test.java" => r"
            import java.util.List;

            class Test {
                void test() {
                    List<Integer> numbers = List.of(1, 2, 3, 4, 5);
                    int sum = numbers.stream()
                        .reduce(0, (ac<|>c, n) -> acc + n);
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["Integer"]
}

// Test 6: sorted() with comparator lambda
lsp_hover_test! {
    name: stream_sorted_comparator_parameters,

    files: {
        "User.java" => r"
            public class User {
                private String name;
                private int age;

                public User(String name, int age) {
                    this.name = name;
                    this.age = age;
                }

                public String getName() { return name; }
                public int getAge() { return age; }
            }
        ",
        "Test.java" => r#"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<User> users = List.of(
                        new User("Alice", 30),
                        new User("Bob", 25)
                    );
                    List<User> sorted = users.stream()
                        .sorted((u<|>1, u2) -> u1.getAge() - u2.getAge())
                        .collect(Collectors.toList());
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["User"]
}

// Test 7: Chained map operations
lsp_hover_test! {
    name: stream_chained_map_inference,

    files: {
        "Test.java" => r#"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<String> names = List.of("Alice", "Bob");
                    List<String> upper = names.stream()
                        .map(n -> n.toUpperCase())
                        .map(upp<|>er -> upper + "!")
                        .collect(Collectors.toList());
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["String"]
}

// Test 8: Generic type inference through stream
lsp_hover_test! {
    name: stream_generic_inference,

    files: {
        "Box.java" => r"
            public class Box<T> {
                private T value;

                public Box(T value) {
                    this.value = value;
                }

                public T getValue() {
                    return value;
                }
            }
        ",
        "Test.java" => r#"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<Box<String>> boxes = List.of(
                        new Box<>("a"),
                        new Box<>("b")
                    );
                    List<String> values = boxes.stream()
                        .map(bo<|>x -> box.getValue())
                        .collect(Collectors.toList());
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["Box"]
}

// Test 9: peek() intermediate operation
lsp_hover_test! {
    name: stream_peek_lambda_parameter,

    files: {
        "Test.java" => r#"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<String> names = List.of("Alice", "Bob");
                    List<String> result = names.stream()
                        .peek(nam<|>e -> System.out.println("Processing: " + name))
                        .collect(Collectors.toList());
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["String"]
}

// Test 10: anyMatch() predicate
lsp_hover_test! {
    name: stream_anymatch_lambda_parameter,

    files: {
        "Test.java" => r"
            import java.util.List;

            class Test {
                void test() {
                    List<Integer> numbers = List.of(1, 2, 3, 4, 5);
                    boolean hasEven = numbers.stream()
                        .anyMatch(<|>n -> n % 2 == 0);
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["Integer"]
}

// Test 11: Complex chaining with type transformations
lsp_hover_test! {
    name: stream_complex_chain_inference,

    files: {
        "User.java" => r"
            public class User {
                private String name;
                private int age;

                public User(String name, int age) {
                    this.name = name;
                    this.age = age;
                }

                public String getName() { return name; }
                public int getAge() { return age; }
            }
        ",
        "Test.java" => r#"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<User> users = List.of(
                        new User("Alice", 30),
                        new User("Bob", 25)
                    );
                    List<Integer> ages = users.stream()
                        .filter(u -> u.getAge() > 20)
                        .map(use<|>r -> user.getAge())
                        .collect(Collectors.toList());
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["User"]
}

// Test 12: collect() with collector
lsp_hover_test! {
    name: stream_collect_inference,

    files: {
        "Test.java" => r#"
            import java.util.List;
            import java.util.Map;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<String> names = List.of("Alice", "Bob", "Charlie");
                    Map<Integer, String> byLength = names.stream()
                        .collect(Collectors.toMap(
                            <|>s -> s.length(),
                            s -> s
                        ));
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["String"]
}

// Test 13: Parallel stream with same inference
lsp_hover_test! {
    name: parallel_stream_lambda_inference,

    files: {
        "Test.java" => r"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<Integer> numbers = List.of(1, 2, 3, 4, 5);
                    List<Integer> squared = numbers.parallelStream()
                        .map(<|>n -> n * n)
                        .collect(Collectors.toList());
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["Integer"]
}

// Test 14: Method reference vs lambda (baseline)
lsp_hover_test! {
    name: stream_method_reference_baseline,

    files: {
        "Test.java" => r#"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<String> names = List.of("Alice", "Bob");
                    // Lambda version for comparison
                    List<Integer> lengths = names.stream()
                        .map(<|>name -> name.length())
                        .collect(Collectors.toList());
                }
            }
        "#
    },

    hover: "Test.java", "<|>",
    contains: ["String"]
}

// Test 15: takeWhile() with lambda (Java 9+)
lsp_hover_test! {
    name: stream_takewhile_lambda_parameter,

    files: {
        "Test.java" => r"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<Integer> numbers = List.of(1, 2, 3, 4, 5);
                    List<Integer> prefix = numbers.stream()
                        .takeWhile(<|>n -> n < 4)
                        .collect(Collectors.toList());
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["Integer"]
}

// Test 16: allMatch() predicate
lsp_hover_test! {
    name: stream_allmatch_lambda_parameter,

    files: {
        "Test.java" => r"
            import java.util.List;

            class Test {
                void test() {
                    List<Integer> numbers = List.of(1, 2, 3, 4, 5);
                    boolean allPositive = numbers.stream()
                        .allMatch(<|>n -> n > 0);
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["Integer"]
}

// Test 17: dropWhile() predicate (Java 9+)
lsp_hover_test! {
    name: stream_dropwhile_lambda_parameter,

    files: {
        "Test.java" => r"
            import java.util.List;
            import java.util.stream.Collectors;

            class Test {
                void test() {
                    List<Integer> numbers = List.of(1, 2, 3, 4, 5);
                    List<Integer> suffix = numbers.stream()
                        .dropWhile(<|>n -> n < 3)
                        .collect(Collectors.toList());
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["Integer"]
}
