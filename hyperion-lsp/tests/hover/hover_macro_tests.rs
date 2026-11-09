// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests demonstrating the hover test macro

// Example 1: Simple hover test with inline marker
lsp_hover_test! {
    name: hover_on_method_call,

    files: {
        "com/example/Main.java" => r"
            package com.example;
            import com.example.model.User;

            public class Main {
                public void test() {
                    get<|>User().getName();
                }

                private User getUser() {
                    return new User();
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

    hover: "com/example/Main.java", "<|>",
    contains: ["User"]
}

// Example 2: Hover on type reference
lsp_hover_test! {
    name: hover_on_type,

    files: {
        "Test.java" => r#"
            class Test {
                void test() {
                    Str<|>ing name = "test";
                }
            }
        "#
    },

    hover: "Test.java", "<|>"
}

// Example 3: Hover on method name
lsp_hover_test! {
    name: hover_on_method_name,

    files: {
        "Foo.java" => r"
            class Foo {
                void te<|>st() {
                    int count = 0;
                }
            }
        "
    },

    hover: "Foo.java", "<|>"
}

// Example 4: Hover on static method receiver
lsp_hover_test! {
    name: hover_on_static_method_receiver,

    files: {
        "Test.java" => r"
            import java.util.Objects;

            class Test {
                void test(String a, String b) {
                    Obje<|>cts.equals(a, b);
                }
            }
        "
    },

    hover: "Test.java", "<|>",
    contains: ["Objects"]
}

// Regression guard: hovering a method return type should resolve the type symbol, not nearby tokens.
lsp_hover_test! {
    name: hover_on_return_type_in_method_signature,

    files: {
        "com/example/CatalogRunner.java" => r"
            package com.example;

            public class CatalogRunner {
                public void start() {}
            }
        ",
        "com/example/Launcher.java" => r"
            package com.example;

            public class Launcher {
                static Catalog<|>Runner startup(String[] args) {
                    return new CatalogRunner();
                }
            }
        "
    },

    hover: "com/example/Launcher.java", "<|>",
    contains: ["CatalogRunner"]
}

// Hover on an imported return type after two startup overloads
// The startup methods deliberately share a name but take different arguments.
lsp_hover_test! {
    name: hover_on_return_type_after_overloads,

    files: {
        "org/example/library/runtime/CatalogRunner.java" => r"
package org.example.library.runtime;

public class CatalogRunner {
    public void start() {}
}
        ",
        "org/example/library/config/CatalogSettings.java" => r"
package org.example.library.config;

public class CatalogSettings {
}
        ",
        "org/example/library/app/CatalogLauncher.java" => r#"
package org.example.library.app;

import org.example.library.config.CatalogSettings;
import org.example.library.runtime.CatalogRunner;

public class CatalogLauncher {
    static final String CATALOG_NAME = "Catalog";

    public static void main(String[] args) {
        startup(args);
    }

    private static void startup(String[] args) {
        CatalogSettings settings = new CatalogSettings();
        startup(args, settings);
    }

    static Catalog<|>Runner startup(String[] args, CatalogSettings config) {
        return new CatalogRunner();
    }
}
        "#
    },

    hover: "org/example/library/app/CatalogLauncher.java", "<|>",
    contains: ["CatalogRunner"]
}

// Diamond operator: hover on the constructor in `new ArrayList<>()` resolves class
lsp_hover_test! {
    name: hover_diamond_constructor_resolves_class,

    files: {
        "com/example/Main.java" => r"
            package com.example;
            import java.util.List;
            import java.util.ArrayList;

            public class Main {
                public void test() {
                    List<String> names = new Array<|>List<>();
                }
            }
        ",
        "java/util/List.java" => r"
            package java.util;
            public interface List<E> {}
        ",
        "java/util/ArrayList.java" => r"
            package java.util;
            public class ArrayList<E> implements List<E> {
                public ArrayList() {}
            }
        "
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["ArrayList"]
}

// A lexical local must not be rebound to a same-named indexed field while
// hover asks the workspace navigation pipeline for canonical declarations.
lsp_hover_test! {
    name: hover_local_shadows_same_named_field,

    files: {
        "Main.java" => r#"
            class Main {
                String value;

                void test() {
                    int value = 1;
                    System.out.println(val<|>ue);
                }
            }
        "#
    },

    hover: "Main.java", "<|>",
    contains: ["int value", "Local variable"]
}
