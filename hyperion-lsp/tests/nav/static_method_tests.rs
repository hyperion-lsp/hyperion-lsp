// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive static method and field tests
// Testing: ClassName.staticMethod(), static field access, static imports
lsp_test_suite! {
    name: "Static Method Resolution",

    // Basic static method call
    test static_method_via_classname {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Utils.help<|>er();
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

    // Static method in same class
    test static_method_same_class {
        files: {
            "com/example/Utils.java" => r"
                package com.example;
                public class Utils {
                    public void test() {
                        Utils.help<|>er();
                    }
                    public static void helper() {}
                }
            "
        },
        nav: "com/example/Utils.java", "<|>" => "Utils.java"
    }

    // Static field access via class name
    test static_field_via_classname {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        String value = Constants.DEFAU<|>LT_VALUE;
                    }
                }
            ",
            "com/example/Constants.java" => r#"
                package com.example;
                public class Constants {
                    public static final String DEFAULT_VALUE = "test";
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Constants.java"
    }

    // Static import method
    test static_import_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Utils.helper;
                public class Main {
                    public void test() {
                        help<|>er();
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

    // Static import wildcard
    test static_import_wildcard {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import static com.example.Utils.*;
                public class Main {
                    public void test() {
                        help<|>er();
                    }
                }
            ",
            "com/example/Utils.java" => r"
                package com.example;
                public class Utils {
                    public static void helper() {}
                    public static void another() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    // Qualified static method call
    test qualified_static_method {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                public class Main {
                    public void test() {
                        com.example.utils.StringUtils.isEmpty<|>("");
                    }
                }
            "#,
            "com/example/utils/StringUtils.java" => r"
                package com.example.utils;
                public class StringUtils {
                    public static boolean isEmpty(String s) {
                        return s == null || s.isEmpty();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "StringUtils.java"
    }

    // Static method with same name as instance method
    test static_method_name_collision {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Factory.crea<|>te();
                    }
                }
            ",
            "com/example/Factory.java" => r"
                package com.example;
                public class Factory {
                    public static Object create() { return null; }
                    public Object create(String type) { return null; }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Factory.java"
    }

    // Static nested class method
    test static_nested_class_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Outer.Inner.help<|>er();
                    }
                }
            ",
            "com/example/Outer.java" => r"
                package com.example;
                public class Outer {
                    public static class Inner {
                        public static void helper() {}
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Outer.java"
    }

    test static_inherited_nested_class_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Child.Nested.help<|>er();
                    }
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public static class Nested {
                        public static void helper() {}
                    }
                }
            ",
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {}
            "
        },
        nav: "com/example/Main.java", "<|>" => "Parent.java"
    }

    // Math static methods
    test math_static_methods {
        files: {
            "com/example/Calculator.java" => r"
                package com.example;
                public class Calculator {
                    public double compute(double a, double b) {
                        return Math.ma<|>x(a, b);
                    }
                }
            ",
            "java/lang/Math.java" => r"
                package java.lang;
                public class Math {
                    public static double max(double a, double b) {
                        return a > b ? a : b;
                    }
                }
            "
        },
        nav: "com/example/Calculator.java", "<|>" => "Math.java"
    }

    // Collections static factory
    test collections_static_factory {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import java.util.*;
                public class Main {
                    public void test() {
                        List<String> list = Collections.emptyLi<|>st();
                    }
                }
            ",
            "java/util/Collections.java" => r"
                package java.util;
                public class Collections {
                    public static <T> List<T> emptyList() {
                        return null;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Collections.java"
    }
}
