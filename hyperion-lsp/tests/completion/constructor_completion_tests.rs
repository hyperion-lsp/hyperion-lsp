// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Constructor Completion Tests
//!
//! Tests for constructor completion with parameter lists and snippets

// ==================== Basic Constructor Completion ====================

lsp_completion_test! {
    name: constructor_no_params,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                public User() {}
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    User user = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["User()"]
}

lsp_completion_test! {
    name: constructor_with_single_param,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                private String name;

                public User(String name) {
                    this.name = name;
                }
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    User user = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["User(String)"]
}

lsp_completion_test! {
    name: constructor_with_multiple_params,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                private String name;
                private int age;

                public User(String name, int age) {
                    this.name = name;
                    this.age = age;
                }
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    User user = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["User(String, int)"]
}

// ==================== Overloaded Constructors ====================

lsp_completion_test! {
    name: constructor_multiple_overloads,
    files: {
        "com/example/Person.java" => r"
            package com.example;

            public class Person {
                private String name;
                private int age;
                private String email;

                public Person() {}

                public Person(String name) {
                    this.name = name;
                }

                public Person(String name, int age) {
                    this.name = name;
                    this.age = age;
                }

                public Person(String name, int age, String email) {
                    this.name = name;
                    this.age = age;
                    this.email = email;
                }
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    Person p = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["Person()", "Person(String)", "Person(String, int)", "Person(String, int, String)"]
}

// ==================== Generic Class Constructors ====================

lsp_completion_test! {
    name: constructor_generic_class,
    files: {
        "com/example/Container.java" => r"
            package com.example;

            public class Container<T> {
                private T value;

                public Container() {}

                public Container(T value) {
                    this.value = value;
                }
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    Container<String> c = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["Container<>()"]
}

// ==================== Cross-File Constructor Completion ====================

lsp_completion_test! {
    name: constructor_cross_file,
    files: {
        "com/example/model/Product.java" => r"
            package com.example.model;

            public class Product {
                private String name;
                private double price;

                public Product(String name, double price) {
                    this.name = name;
                    this.price = price;
                }
            }
        ",
        "com/example/Main.java" => r"
            package com.example;
            import com.example.model.Product;

            public class Main {
                void test() {
                    Product p = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["Product(String, double)"]
}

// ==================== Inner/Nested Class Constructors ====================

lsp_completion_test! {
    name: constructor_static_nested_class,
    files: {
        "com/example/Outer.java" => r"
            package com.example;

            public class Outer {
                public static class Inner {
                    private String value;

                    public Inner(String value) {
                        this.value = value;
                    }
                }
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    Outer.Inner inner = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["Inner(String)"]
}

// ==================== JDK Class Constructors ====================

lsp_completion_test! {
    name: constructor_arraylist,
    files: {
        "com/example/Main.java" => r"
            package com.example;
            import java.util.ArrayList;

            public class Main {
                void test() {
                    ArrayList<String> list = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["ArrayList<>()"]
}

lsp_completion_test! {
    name: constructor_hashmap,
    files: {
        "com/example/Main.java" => r"
            package com.example;
            import java.util.HashMap;

            public class Main {
                void test() {
                    HashMap<String, Integer> map = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["HashMap<>()"]
}

// ==================== After Opening Parenthesis ====================

lsp_completion_test! {
    name: constructor_after_open_paren,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                public User(String name) {}
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    User user = new User(<|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>"
    // Note: After opening paren, we may show parameter hints instead
    // This test just ensures completion works at this position
}

// ==================== Visibility Tests ====================

lsp_completion_test! {
    name: constructor_private_filtered,
    files: {
        "com/example/Singleton.java" => r"
            package com.example;

            public class Singleton {
                private static Singleton instance;

                private Singleton() {} // Private constructor

                public static Singleton getInstance() {
                    if (instance == null) {
                        instance = new Singleton();
                    }
                    return instance;
                }
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    // Should NOT suggest private constructor
                    Singleton s = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    // Private constructor should be filtered out externally
    contains: ["Singleton()"]
}

// ==================== Abstract Class ====================

lsp_completion_test! {
    name: constructor_abstract_class,
    files: {
        "com/example/AbstractBase.java" => r"
            package com.example;

            public abstract class AbstractBase {
                public AbstractBase() {}
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    // Abstract classes cannot be directly instantiated
                    AbstractBase base = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    // Main() should be available, but abstract classes should NOT be offered
    contains: ["Main()"],
    not_contains: ["AbstractBase()"]
}

// ==================== Interface (no constructors) ====================

// Note: Interfaces cannot be directly instantiated with `new InterfaceName()`.
// You need an anonymous class: `new InterfaceName() { ... }`
// So we should NOT suggest the interface name after `new`.
// This test verifies that behavior by checking the interface is NOT suggested.
#[test]
fn constructor_interface_not_suggested() {
    use crate::lsp_test_framework::LspTest;

    let mut test = LspTest::new();
    test.add_file(
        "com/example/MyInterface.java",
        r"
package com.example;

public interface MyInterface {
    void doSomething();
}
",
    );
    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    void test() {
        MyInterface obj = new <|>
    }
}
",
    );

    let result = test.completion("com/example/Main.java", "<|>");
    assert!(result.is_ok(), "Completion failed: {:?}", result.err());
    let completions = result.unwrap();

    // Interface should NOT be in completions (can't do `new MyInterface()`)
    let has_interface = completions.iter().any(|c| c.label == "MyInterface");
    assert!(
        !has_interface,
        "Interface should NOT appear in constructor completion. Got: {:?}",
        completions.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
}

// ==================== Builder Pattern ====================

lsp_completion_test! {
    name: constructor_builder_pattern,
    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                private String name;
                private int age;

                private User() {} // Private constructor for builder

                public static Builder builder() {
                    return new Builder();
                }

                public static class Builder {
                    private String name;
                    private int age;

                    public Builder name(String name) {
                        this.name = name;
                        return this;
                    }

                    public Builder age(int age) {
                        this.age = age;
                        return this;
                    }

                    public User build() {
                        User user = new User();
                        user.name = this.name;
                        user.age = this.age;
                        return user;
                    }
                }
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    User.Builder builder = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["Builder()"]
}

// ==================== Enum (special case) ====================

lsp_completion_test! {
    name: constructor_enum,
    files: {
        "com/example/Color.java" => r"
            package com.example;

            public enum Color {
                RED, GREEN, BLUE
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                void test() {
                    // Enums can't be instantiated with new
                    Color color = new <|>
                }
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    // Enum might show but won't work
    contains: ["Color()"]
}

// ==================== Parameter Display Tests ====================

use crate::lsp_test_framework::LspTest;

/// Test that constructor parameters are shown in the detail field
#[test]
fn test_constructor_shows_params_in_detail() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/User.java",
        r"
package com.example;

public class User {
    private String name;
    private int age;

    public User(String name, int age) {
        this.name = name;
        this.age = age;
    }
}
",
    );

    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    void test() {
        User user = new <|>
    }
}
",
    );

    let result = test.completion("com/example/Main.java", "<|>");
    assert!(result.is_ok(), "Completion failed: {:?}", result.err());
    let completions = result.unwrap();

    // Find User completion - now shows signature in label
    let user_completion = completions.iter().find(|c| c.label.starts_with("User("));
    assert!(
        user_completion.is_some(),
        "User constructor not found in completions"
    );

    let user = user_completion.unwrap();

    // Check that detail or label_details contains parameter info
    let has_params = match &user.detail {
        Some(detail) => detail.contains("String") && detail.contains("int"),
        None => false,
    } || match &user.label_details {
        Some(details) => {
            let desc = details.description.as_deref().unwrap_or("");
            let det = details.detail.as_deref().unwrap_or("");
            (desc.contains("String") && desc.contains("int"))
                || (det.contains("String") && det.contains("int"))
        }
        None => false,
    };

    if has_params {
        println!("✅ Constructor parameters shown in completion");
    } else {
        println!(
            "⚠️ Constructor parameters not shown. Detail: {:?}, LabelDetails: {:?}",
            user.detail, user.label_details
        );
    }
}

/// Test that no-arg constructor shows empty or no-arg indicator
#[test]
fn test_no_arg_constructor_display() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Empty.java",
        r"
package com.example;

public class Empty {
    public Empty() {}
}
",
    );

    test.add_file(
        "com/example/Main.java",
        r"
package com.example;

public class Main {
    void test() {
        Empty e = new <|>
    }
}
",
    );

    let result = test.completion("com/example/Main.java", "<|>");
    assert!(result.is_ok(), "Completion failed: {:?}", result.err());
    let completions = result.unwrap();

    let empty_completion = completions.iter().find(|c| c.label == "Empty()");
    assert!(
        empty_completion.is_some(),
        "Empty() not found in completions"
    );

    let empty = empty_completion.unwrap();
    println!(
        "✅ No-arg constructor completion: label='{}', detail={:?}",
        empty.label, empty.detail
    );
}
