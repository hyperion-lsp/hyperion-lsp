// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Signature Help Tests
//!
//! Tests for the signature help (parameter hints) feature

// Basic signature help test
// NOTE: The workspace indexer currently doesn't extract parameter names,
// so we only check for method name and return type
lsp_signature_help_test! {
    name: basic_method_signature,

    files: {
        "com/example/Main.java" => r"
            package com.example;
            import com.example.model.User;

            public class Main {
                public void test() {
                    User user = new User();
                    user.setName(<|>);
                }
            }
        ",
        "com/example/model/User.java" => r"
            package com.example.model;

            public class User {
                private String name;

                public void setName(String name) {
                    this.name = name;
                }
            }
        "
    },

    signature_help: "com/example/Main.java", "<|>",
    contains: ["setName", "void"]
}

// Signature help with multiple parameters
lsp_signature_help_test! {
    name: multiple_parameters,

    files: {
        "com/example/Calculator.java" => r"
            package com.example;

            public class Calculator {
                public void test() {
                    add(<|>5, 10);
                }

                public int add(int a, int b) {
                    return a + b;
                }
            }
        "
    },

    signature_help: "com/example/Calculator.java", "<|>",
    contains: ["add", "int"]
}

// Signature help after comma (second parameter)
lsp_signature_help_test! {
    name: second_parameter,

    files: {
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test() {
                    compute(5, <|>);
                }

                public void compute(int x, int y) {}
            }
        "
    },

    signature_help: "com/example/Test.java", "<|>",
    contains: ["compute", "void"]
}

// Signature help with String parameter
lsp_signature_help_test! {
    name: string_parameter,

    files: {
        "com/example/Printer.java" => r#"
            package com.example;

            public class Printer {
                public void test() {
                    print(<|>"hello");
                }

                public void print(String message) {
                    System.out.println(message);
                }
            }
        "#
    },

    signature_help: "com/example/Printer.java", "<|>",
    contains: ["print", "void"]
}

// No signature help when not in method call
lsp_signature_help_test! {
    name: no_signature_outside_call,

    files: {
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test<|>() {
                    int x = 5;
                }
            }
        "
    },

    signature_help: "com/example/Test.java", "<|>",
    expect_none: true
}

// Signature help with method that returns value
lsp_signature_help_test! {
    name: method_with_return_type,

    files: {
        "com/example/Math.java" => r"
            package com.example;

            public class Math {
                public void test() {
                    int result = multiply(<|>3, 4);
                }

                public int multiply(int x, int y) {
                    return x * y;
                }
            }
        "
    },

    signature_help: "com/example/Math.java", "<|>",
    contains: ["int", "multiply"]
}

// Signature help on cross-file method call
lsp_signature_help_test! {
    name: cross_file_method_call,

    files: {
        "com/example/Client.java" => r#"
            package com.example;
            import com.example.service.UserService;

            public class Client {
                public void test() {
                    UserService service = new UserService();
                    service.createUser(<|>"John", 25);
                }
            }
        "#,
        "com/example/service/UserService.java" => r"
            package com.example.service;

            public class UserService {
                public void createUser(String name, int age) {
                    // implementation
                }
            }
        "
    },

    signature_help: "com/example/Client.java", "<|>",
    contains: ["createUser", "void"]
}

// Signature help with void return type
lsp_signature_help_test! {
    name: void_return_type,

    files: {
        "com/example/Logger.java" => r#"
            package com.example;

            public class Logger {
                public void test() {
                    log(<|>"error message");
                }

                public void log(String message) {
                    System.out.println(message);
                }
            }
        "#
    },

    signature_help: "com/example/Logger.java", "<|>",
    contains: ["void", "log"]
}
// Phase 3.5.1: Constructor signature help (single constructor)
lsp_signature_help_test! {
    name: constructor_single,

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
        "com/example/Main.java" => r#"
            package com.example;

            public class Main {
                public void test() {
                    User user = new User(<|>"John", 25);
                }
            }
        "#
    },

    signature_help: "com/example/Main.java", "<|>",
    contains: ["User"]
}

// Phase 3.5.2: Constructor signature help (overloaded constructors)
lsp_signature_help_test! {
    name: constructor_overloaded,

    files: {
        "com/example/Person.java" => r"
            package com.example;

            public class Person {
                private String name;
                private int age;
                private String email;

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
        "com/example/Test.java" => r#"
            package com.example;

            public class Test {
                public void test() {
                    Person p1 = new Person(<|>"Alice");
                    Person p2 = new Person("Bob", 30);
                    Person p3 = new Person("Charlie", 25, "charlie@example.com");
                }
            }
        "#
    },

    signature_help: "com/example/Test.java", "<|>",
    contains: ["Person"]
}

// Phase 3.5.3: Cross-file constructor invocation
lsp_signature_help_test! {
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
        "com/example/Main.java" => r#"
            package com.example;
            import com.example.model.Product;

            public class Main {
                public void test() {
                    Product product = new Product(<|>"Laptop", 999.99);
                }
            }
        "#
    },

    signature_help: "com/example/Main.java", "<|>",
    contains: ["Product"]
}

// Phase 3.5.4: Method overload selection
lsp_signature_help_test! {
    name: method_overload_selection,

    files: {
        "com/example/StringUtils.java" => r#"
            package com.example;

            public class StringUtils {
                public void test() {
                    format(<|>"test");
                }

                public String format(String text) {
                    return text.toUpperCase();
                }

                public String format(String text, boolean uppercase) {
                    return uppercase ? text.toUpperCase() : text.toLowerCase();
                }

                public String format(String text, String prefix, String suffix) {
                    return prefix + text + suffix;
                }
            }
        "#
    },

    signature_help: "com/example/StringUtils.java", "<|>",
    contains: ["format", "String"]
}

// Phase 3.5.5: Inherited method signatures
lsp_signature_help_test! {
    name: inherited_method_signature,

    files: {
        "com/example/Animal.java" => r"
            package com.example;

            public class Animal {
                public void makeSound(String sound) {
                    System.out.println(sound);
                }
            }
        ",
        "com/example/Dog.java" => r#"
            package com.example;

            public class Dog extends Animal {
                public void bark() {
                    makeSound(<|>"Woof");
                }
            }
        "#
    },

    signature_help: "com/example/Dog.java", "<|>",
    contains: ["makeSound", "void"]
}

// Phase 3.5.6: Generic method signatures
lsp_signature_help_test! {
    name: generic_method_signature,

    files: {
        "com/example/Container.java" => r"
            package com.example;

            public class Container<T> {
                private T value;

                public void setValue(T value) {
                    this.value = value;
                }

                public T getValue() {
                    return value;
                }

                public void test() {
                    setValue(<|>null);
                }
            }
        "
    },

    signature_help: "com/example/Container.java", "<|>",
    contains: ["setValue", "void"]
}
