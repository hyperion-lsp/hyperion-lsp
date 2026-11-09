// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_test_suite! {
    name: "Method Reference Navigation",

    test static_method_reference {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.stream.Stream;

                public class Main {
                    public void test() {
                        Stream.of("a", "b").map(String::toUpperC<|>ase);
                    }
                }
            "#,
            "java/lang/String.java" => r#"
                package java.lang;
                public class String {
                    public String toUpperCase() {
                        return "";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test constructor_reference {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.ArrayList;
                import java.util.stream.Stream;

                public class Main {
                    public void test() {
                        Stream.of("a").map(ArrayList::<|>new);
                    }
                }
            "#,
            "java/util/ArrayList.java" => r"
                package java.util;
                public class ArrayList {
                    public ArrayList() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "ArrayList.java"
    }

    test instance_method_reference {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test(User user) {
                        Runnable r = user::getName<|>;
                    }
                }
            ",
            "com/example/model/User.java" => r#"
                package com.example.model;
                public class User {
                    public String getName() {
                        return "";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test class_method_reference_to_static {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.Utils;

                public class Main {
                    public void test() {
                        Runnable r = Utils::hel<|>per;
                    }
                }
            ",
            "com/example/Utils.java" => r"
                package com.example;
                public class Utils {
                    public static void helper() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    test constructor_reference_custom_class {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.Person;
                import java.util.function.Supplier;

                public class Main {
                    public void test() {
                        Supplier<Person> factory = Person::<|>new;
                    }
                }
            ",
            "com/example/model/Person.java" => r"
                package com.example.model;
                public class Person {
                    public Person() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Person.java"
    }
}

// Additional standalone tests for edge cases

lsp_goto_test! {
    name: method_reference_with_generics,
    files: {
        "com/example/Test.java" => r"
            package com.example;
            import java.util.List;
            import java.util.ArrayList;

            public class Test {
                public void test() {
                    List<String> list = new ArrayList<>();
                    list.forEach(System.out::print<|>ln);
                }
            }
        ",
        "java/io/PrintStream.java" => r"
            package java.io;
            public class PrintStream {
                public void println(Object x) {}
            }
        "
    },
    nav: "com/example/Test.java", "<|>" => "PrintStream.java"
}

lsp_goto_test! {
    name: array_constructor_reference,
    files: {
        "com/example/Test.java" => r#"
            package com.example;
            import java.util.stream.Stream;

            public class Test {
                public void test() {
                    String[] arr = Stream.of("a").toArray(String[]::n<|>ew);
                }
            }
        "#,
        "java/lang/String.java" => r"
            package java.lang;
            public class String {}
        "
    },
    nav: "com/example/Test.java", "<|>" => "String.java"
}

// The receiver chain of a method reference is walked through the
// canonical field election, so an inherited field does not break it. Reading
// only each owner's declared members stopped at `Config`, which declares
// nothing of its own.
lsp_goto_test! {
    name: method_reference_through_an_inherited_field_in_a_chain,
    files: {
        "com/example/Settings.java" => r"
            package com.example;
            public class Settings {
                public void apply() {}
            }
        ",
        "com/example/BaseConfig.java" => r"
            package com.example;
            public class BaseConfig {
                public Settings settings;
            }
        ",
        "com/example/Config.java" => r"
            package com.example;
            public class Config extends BaseConfig {
            }
        ",
        "com/example/Test.java" => r"
            package com.example;
            import java.util.List;

            public class Test {
                public void run(List<String> items) {
                    Config config = new Config();
                    items.forEach(config.settings::app<|>ly);
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>" => "Settings.java"
}

lsp_goto_test! {
    name: method_reference_on_this,
    files: {
        "com/example/Processor.java" => r"
            package com.example;
            import java.util.List;

            public class Processor {
                public void process(List<String> items) {
                    items.forEach(this::handleI<|>tem);
                }

                private void handleItem(String item) {}
            }
        "
    },
    nav: "com/example/Processor.java", "<|>" => "Processor.java"
}

// Test that method references to primitive types don't crash
lsp_goto_none_test! {
    name: method_reference_builtin_no_source,
    files: {
        "com/example/Test.java" => r"
            package com.example;
            import java.util.stream.IntStream;

            public class Test {
                public void test() {
                    IntStream.range(0, 10).forEach(System.out::prin<|>tln);
                }
            }
        "
    },
    nav: "com/example/Test.java", "<|>"
}

#[test]
fn method_reference_parsing_structure() {
    use hyperion_parser::{Lexer, Parser};

    let source = r"
        class Test {
            void test() {
                Runnable r = String::valueOf;
                Supplier<List> s = ArrayList::new;
            }
        }
    ";

    let tokens: Vec<_> = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);

    assert!(errors.is_empty(), "Parsing should succeed without errors");

    let syntax = hyperion_parser::syntax::SyntaxNode::new_root(green);
    let text = syntax.text().to_string();

    // Verify the source is preserved (lossless parsing)
    assert!(text.contains("String::valueOf"));
    assert!(text.contains("ArrayList::new"));

    println!("✅ Method reference parsing preserves syntax correctly");
}
