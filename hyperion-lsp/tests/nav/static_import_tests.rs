// Copyright 2026 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Static Wildcard Resolution
// Tests that static wildcard imports (import static pkg.Class.*) correctly resolve to methods/fields
// Current behavior: May fail to resolve static members imported via wildcard
// Expected: Should resolve static members to their defining class

lsp_test_suite! {
    name: "Static Wildcard Import Resolution",

    test static_method_wildcard {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Utils.*;

                public class Main {
                    public void test() {
                        hel<|>per();
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

    test static_field_wildcard {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Constants.*;

                public class Main {
                    public void test() {
                        int x = MAX_V<|>ALUE;
                    }
                }
            ",
            "com/example/Constants.java" => r"
                package com.example;

                public class Constants {
                    public static final int MAX_VALUE = 100;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Constants.java"
    }

    test multiple_static_wildcard_imports {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import static com.example.MathUtils.*;
                import static com.example.StringUtils.*;

                public class Main {
                    public void test() {
                        int result = add(1, 2);
                        String s = toUp<|>per("test");
                    }
                }
            "#,
            "com/example/MathUtils.java" => r"
                package com.example;

                public class MathUtils {
                    public static int add(int a, int b) { return a + b; }
                }
            ",
            "com/example/StringUtils.java" => r"
                package com.example;

                public class StringUtils {
                    public static String toUpper(String s) { return s.toUpperCase(); }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "StringUtils.java"
    }

    test static_wildcard_with_explicit_import {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Utils.*;
                import static com.example.Utils.helper;

                public class Main {
                    public void test() {
                        hel<|>per();
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

    test static_wildcard_overloaded_methods {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import static com.example.Utils.*;

                public class Main {
                    public void test() {
                        format<|>("test");
                        format("test", 123);
                    }
                }
            "#,
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public static String format(String s) { return s; }
                    public static String format(String s, int n) { return s + n; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    test static_wildcard_nested_class {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Outer.Inner.*;

                public class Main {
                    public void test() {
                        stati<|>cMethod();
                    }
                }
            ",
            "com/example/Outer.java" => r"
                package com.example;

                public class Outer {
                    public static class Inner {
                        public static void staticMethod() {}
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Outer.java"
    }
}

lsp_hover_test! {
    name: static_wildcard_hover_method,

    files: {
        "com/example/Main.java" => r"
            package com.example;
            import static com.example.Utils.*;

            public class Main {
                public void test() {
                    hel<|>per();
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

    hover: "com/example/Main.java", "<|>",
    contains: ["static", "void", "helper"]
}

lsp_hover_test! {
    name: static_wildcard_hover_field,

    files: {
        "com/example/Main.java" => r"
            package com.example;
            import static com.example.Constants.*;

            public class Main {
                public void test() {
                    int x = MAX_V<|>ALUE;
                }
            }
        ",
        "com/example/Constants.java" => r"
            package com.example;

            public class Constants {
                public static final int MAX_VALUE = 100;
            }
        "
    },

    hover: "com/example/Main.java", "<|>",
    contains: ["static", "final", "int", "MAX_VALUE"]
}

// ===== Explicit (non-wildcard) static import tests =====

lsp_test_suite! {
    name: "Explicit Static Import Resolution",

    test explicit_static_method_import {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Utils.helper;

                public class Main {
                    public void test() {
                        hel<|>per();
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

    test explicit_static_field_import {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Constants.MAX_VALUE;

                public class Main {
                    public void test() {
                        int x = MAX_V<|>ALUE;
                    }
                }
            ",
            "com/example/Constants.java" => r"
                package com.example;

                public class Constants {
                    public static final int MAX_VALUE = 100;
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Constants.java"
    }

    test explicit_static_method_import_different_package {
        files: {
            "com/example/app/Main.java" => r"
                package com.example.app;
                import static com.example.util.Helpers.doWork;

                public class Main {
                    public void test() {
                        do<|>Work();
                    }
                }
            ",
            "com/example/util/Helpers.java" => r"
                package com.example.util;

                public class Helpers {
                    public static void doWork() {}
                }
            "
        },
        nav: "com/example/app/Main.java", "<|>" => "Helpers.java"
    }

    test explicit_static_import_on_import_line {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Utils.hel<|>per;

                public class Main {
                    public void test() {
                        helper();
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
}
