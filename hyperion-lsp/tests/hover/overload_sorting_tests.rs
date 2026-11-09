// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Method Overload Sorting
// Tests that overloaded methods are sorted consistently by signature for completion/hover
// Current behavior: Overloads may appear in random order or inconsistent between requests
// Expected: Should return overloads in deterministic order (by parameter count, then parameter types)

lsp_test_suite! {
    name: "Method Overload Sorting",

    test overload_by_parameter_count {
        files: {
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public static void format() {}
                    public static void format(String s) {}
                    public static void format(String s, int n) {}
                    public static void format(String s, int n, boolean b) {}
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Utils.form<|>at("test");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    test overload_by_parameter_types {
        files: {
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public static void process(int value) {}
                    public static void process(String value) {}
                    public static void process(double value) {}
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Utils.proce<|>ss(42);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    test constructor_overloads {
        files: {
            "com/example/User.java" => r"
                package com.example;

                public class User {
                    public User() {}
                    public User(String name) {}
                    public User(String name, int age) {}
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        User user = new Us<|>er("John");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test overload_with_varargs {
        files: {
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public static void print(String msg) {}
                    public static void print(String msg, Object... args) {}
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Utils.pri<|>nt("Hello", "World");
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    test overload_with_generics {
        files: {
            "com/example/Container.java" => r"
                package com.example;

                public class Container {
                    public <T> T get(Class<T> clazz) { return null; }
                    public <T> T get(Class<T> clazz, T defaultValue) { return null; }
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(Container c) {
                        String s = c.g<|>et(String.class);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Container.java"
    }

    test overload_with_arrays {
        files: {
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public static void process(int value) {}
                    public static void process(int[] values) {}
                    public static void process(int[][] matrix) {}
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        int[] arr = {1, 2, 3};
                        Utils.proce<|>ss(arr);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    test inherited_overloads {
        files: {
            "com/example/Base.java" => r"
                package com.example;

                public class Base {
                    public void method(int value) {}
                }
            ",
            "com/example/Derived.java" => r"
                package com.example;

                public class Derived extends Base {
                    public void method(String value) {}
                    public void method(int value, String name) {}
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test(Derived d) {
                        d.meth<|>od(42);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Base.java"
    }

    test interface_overloads {
        files: {
            "com/example/Processor.java" => r"
                package com.example;

                public interface Processor {
                    void process(String input);
                    void process(String input, int flags);
                    void process(String input, int flags, boolean verbose);
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test(Processor p) {
                        p.proce<|>ss("test", 0);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Processor.java"
    }

    test overload_with_wrapper_types {
        files: {
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public static void handle(int value) {}
                    public static void handle(Integer value) {}
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Utils.hand<|>le(42);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    test overload_with_null_parameter {
        files: {
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public static void process(String value) {}
                    public static void process(Object value) {}
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        Utils.proce<|>ss(null);
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }
}

lsp_hover_test! {
    name: hover_shows_all_overloads,

    files: {
        "com/example/Utils.java" => r"
            package com.example;

            public class Utils {
                public static void format() {}
                public static void format(String s) {}
                public static void format(String s, int n) {}
            }
        ",
        "com/example/Main.java" => r#"
            package com.example;

            public class Main {
                public void test() {
                    Utils.form<|>at("test");
                }
            }
        "#
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["format", "String"]
}

lsp_hover_test! {
    name: hover_constructor_overloads,

    files: {
        "com/example/User.java" => r"
            package com.example;

            public class User {
                public User() {}
                public User(String name) {}
                public User(String name, int age) {}
            }
        ",
        "com/example/Main.java" => r#"
            package com.example;

            public class Main {
                public void test() {
                    User user = new Us<|>er("John");
                }
            }
        "#
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["User"]
}

lsp_hover_test! {
    name: hover_same_arity_overload_prefers_argument_types,

    files: {
        "com/example/Utils.java" => r"
            package com.example;

            public class Utils {
                public static void process(int value) {}
                public static void process(String value) {}
            }
        ",
        "com/example/Main.java" => r#"
            package com.example;

            public class Main {
                public void test() {
                    Utils.pro<|>cess("hello");
                }
            }
        "#
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["process", "String"]
}

lsp_hover_test! {
    name: hover_invalid_typed_call_does_not_guess_an_overload,

    files: {
        "com/example/Utils.java" => r"
            package com.example;

            public class Utils {
                public static void process(int count) {}
                public static void process(long distance) {}
            }
        ",
        "com/example/Main.java" => r#"
            package com.example;

            public class Main {
                public void test() {
                    Utils.pro<|>cess("not a number");
                }
            }
        "#
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["process", "No applicable overload"]
}
