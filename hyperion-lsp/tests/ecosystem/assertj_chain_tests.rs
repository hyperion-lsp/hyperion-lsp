// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for AssertJ-style fluent API chains with static imports
// Reproduces the issue where:
// 1. assertThatThrownBy shows "10 method definitions found"
// 2. Chained methods like .hasMessage() have no type information
lsp_test_suite! {
    name: "AssertJ Chain Resolution",

    // Basic fluent chain with static import
    test static_import_fluent_chain {
        files: {
            "com/example/Test.java" => r#"
                package com.example;
                import static com.example.Assertions.assertThatThrownBy;

                public class Test {
                    public void test() {
                        assertThatThrownBy<|>(() -> { throw new Exception(); })
                            .hasMessage("test");
                    }
                }
            "#,
            "com/example/Assertions.java" => r"
                package com.example;

                public class Assertions {
                    public static ThrowableAssert assertThatThrownBy(ThrowingCallable callable) {
                        return new ThrowableAssert();
                    }
                }
            ",
            "com/example/ThrowableAssert.java" => r"
                package com.example;

                public class ThrowableAssert {
                    public ThrowableAssert hasMessage(String message) {
                        return this;
                    }
                    public ThrowableAssert isInstanceOf(Class<?> clazz) {
                        return this;
                    }
                }
            ",
            "com/example/ThrowingCallable.java" => r"
                package com.example;

                public interface ThrowingCallable {
                    void call() throws Exception;
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "Assertions.java"
    }

    // Chained method after static import
    test static_import_chain_second_method {
        files: {
            "com/example/Test.java" => r#"
                package com.example;
                import static com.example.Assertions.assertThatThrownBy;

                public class Test {
                    public void test() {
                        assertThatThrownBy(() -> { throw new Exception(); })
                            .hasMess<|>age("test");
                    }
                }
            "#,
            "com/example/Assertions.java" => r"
                package com.example;

                public class Assertions {
                    public static ThrowableAssert assertThatThrownBy(ThrowingCallable callable) {
                        return new ThrowableAssert();
                    }
                }
            ",
            "com/example/ThrowableAssert.java" => r"
                package com.example;

                public class ThrowableAssert {
                    public ThrowableAssert hasMessage(String message) {
                        return this;
                    }
                }
            ",
            "com/example/ThrowingCallable.java" => r"
                package com.example;

                public interface ThrowingCallable {
                    void call() throws Exception;
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "ThrowableAssert.java"
    }

    // Third method in chain
    test static_import_chain_third_method {
        files: {
            "com/example/Test.java" => r#"
                package com.example;
                import static com.example.Assertions.assertThatThrownBy;

                public class Test {
                    public void test() {
                        assertThatThrownBy(() -> { throw new Exception(); })
                            .hasMessage("test")
                            .isInstance<|>Of(IllegalStateException.class);
                    }
                }
            "#,
            "com/example/Assertions.java" => r"
                package com.example;

                public class Assertions {
                    public static ThrowableAssert assertThatThrownBy(ThrowingCallable callable) {
                        return new ThrowableAssert();
                    }
                }
            ",
            "com/example/ThrowableAssert.java" => r"
                package com.example;

                public class ThrowableAssert {
                    public ThrowableAssert hasMessage(String message) {
                        return this;
                    }
                    public ThrowableAssert isInstanceOf(Class<?> clazz) {
                        return this;
                    }
                }
            ",
            "com/example/ThrowingCallable.java" => r"
                package com.example;

                public interface ThrowingCallable {
                    void call() throws Exception;
                }
            "
        },
        nav: "com/example/Test.java", "<|>" => "ThrowableAssert.java"
    }
}

lsp_hover_test! {
    name: hover_static_import_shows_specific_method,

    files: {
        "com/example/Test.java" => r"
            package com.example;
            import static com.example.Assertions.assertThatThrownBy;

            public class Test {
                public void test() {
                    assertThatThr<|>ownBy(() -> { throw new Exception(); });
                }
            }
        ",
        "com/example/Assertions.java" => r"
            package com.example;

            public class Assertions {
                public static ThrowableAssert assertThatThrownBy(ThrowingCallable callable) {
                    return new ThrowableAssert();
                }
            }
        ",
        "com/example/ThrowableAssert.java" => r"
            package com.example;

            public class ThrowableAssert {
                public ThrowableAssert hasMessage(String message) {
                    return this;
                }
            }
        ",
        "com/example/ThrowingCallable.java" => r"
            package com.example;

            public interface ThrowingCallable {
                void call() throws Exception;
            }
        "
    },

    hover: "com/example/Test.java", "<|>",
    contains: ["assertThatThrownBy", "Assertions"]
}

lsp_hover_test! {
    name: hover_chained_method_shows_type,

    files: {
        "com/example/Test.java" => r#"
            package com.example;
            import static com.example.Assertions.assertThatThrownBy;

            public class Test {
                public void test() {
                    assertThatThrownBy(() -> { throw new Exception(); })
                        .hasMe<|>ssage("test");
                }
            }
        "#,
        "com/example/Assertions.java" => r"
            package com.example;

            public class Assertions {
                public static ThrowableAssert assertThatThrownBy(ThrowingCallable callable) {
                    return new ThrowableAssert();
                }
            }
        ",
        "com/example/ThrowableAssert.java" => r"
            package com.example;

            public class ThrowableAssert {
                public ThrowableAssert hasMessage(String message) {
                    return this;
                }
            }
        ",
        "com/example/ThrowingCallable.java" => r"
            package com.example;

            public interface ThrowingCallable {
                void call() throws Exception;
            }
        "
    },

    hover: "com/example/Test.java", "<|>",
    contains: ["hasMessage", "ThrowableAssert"]
}
