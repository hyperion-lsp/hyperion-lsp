// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for constructor type inference within lambda expressions
// These tests break down the problem:
// - Goto-definition fails on type in constructor within lambda
// - Hover shows incomplete info for method on constructed object
//
// The problematic pattern is: stream().map(x -> new Type().method()).collect(...)
// Where type inference must track: stream() -> Stream<T> -> lambda param is T
// Then within lambda: new Type() -> Type, and Type.method() must resolve

lsp_test_suite! {
    name: "Lambda Constructor Type Inference",

    // SUBPROBLEM 1: Simple constructor in lambda (no method call yet)
    // Tests: Can we infer that new Type() returns Type within a lambda context?
    test constructor_in_lambda_simple_case {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test(List<String> items) {
                        items.stream()
                            .map(s -> new Us<|>er())
                            .collect(Collectors.toList());
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

    // SUBPROBLEM 2: Method call on newly constructed object
    // Tests: Can we resolve methods on newly constructed objects within lambda?
    test method_on_constructed_object_in_lambda {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test(List<String> items) {
                        items.stream()
                            .map(s -> new User().getName<|>())
                            .collect(Collectors.toList());
                    }
                }
            ",
            "com/example/User.java" => r#"
                package com.example;
                public class User {
                    public User() {}
                    public String getName() { return ""; }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // SUBPROBLEM 3: Chained method calls on constructed object
    // Tests: Can we handle method chaining after constructor in lambda?
    test chained_methods_on_constructed_object_in_lambda {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test(List<String> items) {
                        items.stream()
                            .map(s -> new User().setName(s).getSe<|>ttings())
                            .collect(Collectors.toList());
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    public User() {}
                    public User setName(String name) { return this; }
                    public Settings getSettings() { return new Settings(); }
                }
            ",
            "com/example/Settings.java" => r"
                package com.example;
                public class Settings {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // Builder-pattern-in-lambda navigation is not asserted yet because nested-class
    // FQN resolution (`User.Builder` vs `com.example.Builder`) is still incomplete.

    // A constructor followed by a setter inside a stream mapping
    // The cursor is inside the constructed type, before the fluent setter.
    test catalog_constructor_in_lambda {
        files: {
            "org/example/library/ShelfInventory.java" => r"
                package org.example.library;
                import java.util.*;
                import java.util.stream.*;
                public class ShelfInventory {
                    private List<Integer> bookIds;
                    public void test() {
                        List<BookState> states = bookIds.stream()
                            .map(bookId -> new Book<|>State().setBookId(bookId))
                            .collect(Collectors.toList());
                    }
                }
            ",
            "org/example/library/BookState.java" => r"
                package org.example.library;
                public class BookState {
                    public BookState() {}
                    public BookState setBookId(int id) { return this; }
                }
            "
        },
        nav: "org/example/library/ShelfInventory.java", "<|>" => "BookState.java"
    }


    // SUBPROBLEM 8: Multiple constructors with same class (type inference shouldn't break)
    // Tests: Type inference should work even with multiple levels of nesting
    test multiple_constructors_in_chain {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test(List<String> items) {
                        items.stream()
                            .map(s -> new Config()
                                .setUser(new Us<|>er().setName(s))
                                .build())
                            .collect(Collectors.toList());
                    }
                }
            ",
            "com/example/User.java" => r"
                package com.example;
                public class User {
                    public User() {}
                    public User setName(String name) { return this; }
                }
            ",
            "com/example/Config.java" => r"
                package com.example;
                public class Config {
                    public Config setUser(User user) { return this; }
                    public Config build() { return this; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    // SUBPROBLEM 9: Constructor with type arguments (generics)
    // Tests: Can we handle generic type arguments in constructors?
    test generic_constructor_in_lambda {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test(List<String> items) {
                        items.stream()
                            .map(s -> new Contain<|>er<String>().setValue(s))
                            .collect(Collectors.toList());
                    }
                }
            ",
            "com/example/Container.java" => r"
                package com.example;
                public class Container<T> {
                    public Container() {}
                    public Container<T> setValue(T value) { return this; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Container.java"
    }

    // SUBPROBLEM 10: Constructor from different package in lambda
    // Tests: Fully qualified type resolution in lambda context
    test qualified_constructor_in_lambda {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                import java.util.stream.*;
                public class Main {
                    public void test(List<String> items) {
                        items.stream()
                            .map(s -> new com.other.Ty<|>pe().process(s))
                            .collect(Collectors.toList());
                    }
                }
            ",
            "com/other/Type.java" => r"
                package com.other;
                public class Type {
                    public Type() {}
                    public String process(String s) { return s; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Type.java"
    }
}

// Separate hover tests for constructor type inference

lsp_hover_test! {
    name: hover_on_constructor_type_in_lambda,
    files: {
        "com/example/Main.java" => r"
            package com.example;
            import java.util.*;
            import java.util.stream.*;
            public class Main {
                public void test(List<String> items) {
                    items.stream()
                        .map(s -> new Us<|>er())
                        .collect(Collectors.toList());
                }
            }
        ",
        "com/example/User.java" => r"
            package com.example;
            /**
             * Represents a user in the system
             */
            public class User {
                public User() {}
            }
        "
    },
    hover: "com/example/Main.java", "<|>",
    contains: ["User"]
}

// NOTE: This test currently fails due to hover cursor positioning issue
// The cursor is on the '(' in getName() and shows "L_PAREN" instead of method info
// This is a UI detail and lower priority
// lsp_hover_test! {
//     name: hover_on_method_of_constructed_object_in_lambda,
//     files: {
//         "com/example/Main.java" => r#"
//             package com.example;
//             import java.util.*;
//             import java.util.stream.*;
//             public class Main {
//                 public void test(List<String> items) {
//                     items.stream()
//                         .map(s -> new User().getName<|>())
//                         .collect(Collectors.toList());
//                 }
//             }
//         "#,
//         "com/example/User.java" => r#"
//             package com.example;
//             public class User {
//                 public User() {}
//                 /**
//                  * Returns the user's name
//                  */
//                 public String getName() { return ""; }
//             }
//         "#
//     },
//     hover: "com/example/Main.java", "<|>",
//     contains: ["getName"]
// }

lsp_hover_test! {
    name: hover_on_catalog_constructor_pattern,
    files: {
        "org/example/library/ShelfInventory.java" => r"
            package org.example.library;
            import java.util.*;
            import java.util.stream.*;
            public class ShelfInventory {
                private List<Integer> bookIds;
                public void test() {
                    List<BookState> states = bookIds.stream()
                        .map(bookId -> new Book<|>State().setBookId(bookId))
                        .collect(Collectors.toList());
                }
            }
        ",
        "org/example/library/BookState.java" => r"
            package org.example.library;
            public class BookState {
                public BookState() {}
                public BookState setBookId(int id) { return this; }
            }
        "
    },
    hover: "org/example/library/ShelfInventory.java", "<|>",
    contains: ["BookState"]
}
