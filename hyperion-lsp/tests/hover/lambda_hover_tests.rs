// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Lambda parameter hover tests
// Tests that hovering on lambda parameters shows inferred type information

lsp_hover_test! {
    name: hover_consumer_lambda_param,

    files: {
        "test/Main.java" => r#"
            package test;
            import java.util.function.Consumer;

            public class Main {
                public void test() {
                    Consumer<String> c = <|>s -> System.out.println(s);
                    c.accept("hello");
                }
            }
        "#
    },

    hover: "test/Main.java", "<|>",
    contains: ["String", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_function_lambda_param,

    files: {
        "test/Main.java" => r#"
            package test;
            import java.util.function.Function;

            public class Main {
                public void test() {
                    Function<String, Integer> f = <|>s -> s.length();
                    f.apply("test");
                }
            }
        "#
    },

    hover: "test/Main.java", "<|>",
    contains: ["String", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_bifunction_lambda_first_param,

    files: {
        "test/Main.java" => r#"
            package test;
            import java.util.function.BiFunction;

            public class Main {
                public void test() {
                    BiFunction<String, Integer, Boolean> f = (<|>s, i) -> s.length() > i;
                    f.apply("hello", 3);
                }
            }
        "#
    },

    hover: "test/Main.java", "<|>",
    contains: ["String", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_bifunction_lambda_second_param,

    files: {
        "test/Main.java" => r#"
            package test;
            import java.util.function.BiFunction;

            public class Main {
                public void test() {
                    BiFunction<String, Integer, Boolean> f = (s,<|>i) -> s.length() > i;
                    f.apply("hello", 3);
                }
            }
        "#
    },

    hover: "test/Main.java", "<|>",
    contains: ["Integer", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_predicate_lambda_param,

    files: {
        "test/Main.java" => r#"
            package test;
            import java.util.function.Predicate;

            public class Main {
                public void test() {
                    Predicate<String> isEmpty = <|>s -> s.isEmpty();
                    isEmpty.test("hello");
                }
            }
        "#
    },

    hover: "test/Main.java", "<|>",
    contains: ["String", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_stream_map_lambda_param,

    files: {
        "test/Main.java" => r#"
            package test;
            import java.util.List;

            public class Main {
                public void test() {
                    List<String> names = List.of("Alice", "Bob");
                    names.stream().map(<|>s -> s.toUpperCase()).toList();
                }
            }
        "#
    },

    hover: "test/Main.java", "<|>",
    contains: ["String", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_stream_filter_lambda_param,

    files: {
        "test/Main.java" => r#"
            package test;
            import java.util.List;

            public class Main {
                public void test() {
                    List<String> names = List.of("Alice", "Bob");
                    names.stream().filter(<|>s -> s.startsWith("A")).toList();
                }
            }
        "#
    },

    hover: "test/Main.java", "<|>",
    contains: ["String", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_lambda_param_after_multi_call_generic_chain,

    files: {
        "test/Main.java" => r"
            package test;

            import java.util.Comparator;
            import java.util.List;
            import java.util.Set;

            class Coord {}

            class Board {
                List<Set<Coord>> getChunks() {
                    return null;
                }
            }

            public class Main {
                void test(Board board, Comparator<Set<Coord>> comparator) {
                    board.getChunks().stream().max(comparator)
                        .map(<|>chunk -> chunk.size());
                }
            }
        "
    },

    hover: "test/Main.java", "<|>",
    contains: ["java.util.Set<test.Coord>", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_lambda_param_does_not_leak_local_from_other_method,

    files: {
        "test/Main.java" => r"
            package test;

            import java.util.List;

            public class Main {
                private List<Integer> values;

                void unrelated() {
                    List<String> values = null;
                }

                void test() {
                    values.stream().map(<|>value -> value + 1);
                }
            }
        "
    },

    hover: "test/Main.java", "<|>",
    contains: ["java.lang.Integer", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_nested_lambda_outer_param,

    files: {
        "test/Main.java" => r#"
            package test;
            import java.util.function.Function;
            import java.util.function.Consumer;

            public class Main {
                public void test() {
                    Function<String, Consumer<String>> factory =
                        <|>prefix -> msg -> System.out.println(prefix + ": " + msg);
                    factory.apply("INFO").accept("test");
                }
            }
        "#
    },

    hover: "test/Main.java", "<|>",
    contains: ["String", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_nested_lambda_inner_param,

    files: {
        "test/Main.java" => r#"
            package test;
            import java.util.function.Function;
            import java.util.function.Consumer;

            public class Main {
                public void test() {
                    Function<String, Consumer<String>> factory =
                        prefix -> <|>msg -> System.out.println(prefix + ": " + msg);
                    factory.apply("INFO").accept("test");
                }
            }
        "#
    },

    hover: "test/Main.java", "<|>",
    contains: ["String", "Lambda parameter"]
}

// Edge cases

lsp_hover_test! {
    name: hover_lambda_with_generic_user_class,

    files: {
        "test/Main.java" => r"
            package test;
            import java.util.function.Consumer;
            import test.model.User;

            public class Main {
                public void test() {
                    Consumer<User> processor = <|>user -> user.process();
                }
            }
        ",
        "test/model/User.java" => r"
            package test.model;

            public class User {
                public void process() {}
            }
        "
    },

    hover: "test/Main.java", "<|>",
    contains: ["User", "Lambda parameter"]
}

// Custom method lambda inference tests
// These test inferring lambda parameter types from custom methods
// that take functional interfaces as parameters (not just standard Stream/Optional methods)

lsp_hover_test! {
    name: hover_custom_method_consumer_lambda,

    files: {
        "test/Main.java" => r"
            package test;
            import test.service.UserService;
            import test.model.User;

            public class Main {
                public void test() {
                    UserService service = new UserService();
                    service.processUser(<|>user -> user.getName());
                }
            }
        ",
        "test/service/UserService.java" => r"
            package test.service;
            import java.util.function.Consumer;
            import test.model.User;

            public class UserService {
                public void processUser(Consumer<User> processor) {
                    processor.accept(new User());
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

    hover: "test/Main.java", "<|>",
    contains: ["User", "Lambda parameter"]
}

lsp_hover_test! {
    name: hover_custom_method_function_lambda,

    files: {
        "test/Main.java" => r"
            package test;
            import test.service.Transformer;
            import test.model.User;
            import test.model.UserDto;

            public class Main {
                public void test() {
                    Transformer transformer = new Transformer();
                    UserDto dto = transformer.transform(<|>user -> new UserDto(user.getName()));
                }
            }
        ",
        "test/service/Transformer.java" => r"
            package test.service;
            import java.util.function.Function;
            import test.model.User;
            import test.model.UserDto;

            public class Transformer {
                public UserDto transform(Function<User, UserDto> mapper) {
                    return mapper.apply(new User());
                }
            }
        ",
        "test/model/User.java" => r#"
            package test.model;

            public class User {
                public String getName() { return ""; }
            }
        "#,
        "test/model/UserDto.java" => r"
            package test.model;

            public class UserDto {
                public UserDto(String name) {}
            }
        "
    },

    hover: "test/Main.java", "<|>",
    contains: ["User", "Lambda parameter"]
}
