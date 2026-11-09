// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Regression test for: Symbol deduplication bug
//
// When a FIELD and CLASS had the same simple name, deduplication by name
// would "steal" the slot from the CLASS before type filtering happened,
// causing the CLASS to be excluded.
//
// Example: A field `systemConfiguration` would prevent the class
// `SystemConfiguration` from appearing in completions.
lsp_completion_test! {
    name: test_deduplication_field_does_not_block_class,
    files: {
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                private String systemConfiguration;

                public void test() {
                    SystemConf<|>
                }
            }
        ",
        "com/example/SystemConfiguration.java" => r"
            package com.example;

            public class SystemConfiguration {
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["SystemConfiguration"]
}

// Regression test for: Multi-keystroke completion
//
// When user typed a full word like "SystemConf" in one go, Neovim would
// send ONE completion request at the beginning (with just "S"), then filter
// locally and never ask the server again. This caused no results to appear.
//
// The fix: Return is_incomplete=true to tell client to re-query as user types.
lsp_completion_test! {
    name: test_multi_keystroke_completion_with_prefix_growth,
    files: {
        "com/example/Client.java" => r"
            package com.example;

            public class Client {
                public void connect() {
                    DatabaseCon<|>
                }
            }
        ",
        "com/example/DatabaseConnection.java" => r"
            package com.example;

            public class DatabaseConnection {
                public void open() {}
            }
        "
    },
    complete: "com/example/Client.java", "<|>",
    contains: ["DatabaseConnection"]
}

// Regression test for: Auto-import with ambiguous class names
//
// When multiple classes have the same simple name (e.g., "User" in different packages),
// auto-import would be skipped entirely. The fix uses FQN from metadata instead of
// re-querying by simple name, so auto-import works even with ambiguity.
lsp_completion_test! {
    name: test_auto_import_with_ambiguous_class_names,
    files: {
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                public void test() {
                    User<|>
                }
            }
        ",
        "com/example/User.java" => r"
            package com.example;

            public class User {
            }
        ",
        "com/apache/User.java" => r"
            package com.apache;

            public class User {
            }
        "
    },
    complete: "com/example/Main.java", "<|>",
    contains: ["User"]
}

// Regression test for: Workspace type completions work for partially-typed names
//
// Ensures that as user types more characters, completions are updated.
// This tests the is_incomplete flag behavior indirectly through the framework.
lsp_completion_test! {
    name: test_workspace_types_partial_matching,
    files: {
        "com/example/Service.java" => r"
            package com.example;

            public class Service {
                public void work() {
                    StringUtil<|>
                }
            }
        ",
        "com/example/StringUtils.java" => r"
            package com.example;

            public class StringUtils {
                public static String trim(String s) { return s; }
            }
        "
    },
    complete: "com/example/Service.java", "<|>",
    contains: ["StringUtils"]
}

// Regression test for: Type filtering before deduplication
//
// Ensures that non-type symbols (methods, fields, etc.) don't interfere
// with type completions through the deduplication mechanism.
lsp_completion_test! {
    name: test_type_filtering_happens_before_dedup,
    files: {
        "com/example/Factory.java" => r"
            package com.example;

            public class Factory {
                private Configuration configuration;

                public void build() {
                    Configuration<|>
                }
            }
        ",
        "com/example/Configuration.java" => r"
            package com.example;

            public class Configuration {
                public void load() {}
            }
        "
    },
    complete: "com/example/Factory.java", "<|>",
    contains: ["Configuration"]
}

// Test: Cross-package type completions still work
//
// Ensures completions work across different package structures
lsp_completion_test! {
    name: test_cross_package_type_completion,
    files: {
        "com/app/handler/Handler.java" => r"
            package com.app.handler;

            public class Handler {
                public void process() {
                    AppConfig<|>
                }
            }
        ",
        "com/app/config/AppConfig.java" => r"
            package com.app.config;

            public class AppConfig {
            }
        "
    },
    complete: "com/app/handler/Handler.java", "<|>",
    contains: ["AppConfig"]
}

// Regression test: member access completions should persist as prefix grows
// Example: foo.getBar() should still be suggested when typing foo.g
lsp_completion_test! {
    name: test_member_access_prefix_filtering,
    files: {
        "com/example/Foo.java" => r"
            package com.example;

            public class Foo {
                public Bar getBar() { return new Bar(); }
            }
        ",
        "com/example/Bar.java" => r"
            package com.example;

            public class Bar {
            }
        ",
        "com/example/Test.java" => r"
            package com.example;

            public class Test {
                public void test() {
                    Foo foo = new Foo();
                    foo.g<|>etBar();
                }
            }
        "
    },
    complete: "com/example/Test.java", "<|>",
    contains: ["getBar"]
}
