// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

lsp_test_suite! {
    name: "Method Navigation - Cross File",

    test simple_method_call_cross_file {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.get<|>Name();
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

    test static_method_call_cross_file {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.Utils;

                public class Main {
                    public void test() {
                        Utils.helpe<|>r();
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

    test qualified_method_call_cross_file {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        System.out.print<|>ln("test");
                    }
                }
            "#,
            "java/lang/System.java" => r"
                package java.lang;
                import java.io.PrintStream;

                public class System {
                    public static PrintStream out = null;
                }
            ",
            "java/io/PrintStream.java" => r"
                package java.io;
                public class PrintStream {
                    public void println(String x) {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "PrintStream.java"
    }

    test chained_method_calls_cross_file {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test() {
                        User user = new User();
                        user.getName().trim<|>();
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
            "#,
            "java/lang/String.java" => r"
                package java.lang;
                public class String {
                    public String trim() {
                        return this;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "String.java"
    }

    test chained_overload_uses_intermediate_argument_types {
        files: {
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    public void test() {
                        Factory factory = new Factory();
                        factory.choose(1).acce<|>pt("result");
                    }
                }
            "#,
            "com/example/Factory.java" => r"
                package com.example;

                public class Factory {
                    public BoxedChoice choose(Integer value) {
                        return new BoxedChoice();
                    }

                    public WidenedChoice choose(long value) {
                        return new WidenedChoice();
                    }
                }
            ",
            "com/example/BoxedChoice.java" => r"
                package com.example;

                public class BoxedChoice {
                    public void accept(String boxed) {}
                }
            ",
            "com/example/WidenedChoice.java" => r"
                package com.example;

                public class WidenedChoice {
                    public void accept(String widened) {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "WidenedChoice.java"
    }

    test method_on_field_cross_file {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    private User user;

                    public void test() {
                        user.getEm<|>ail();
                    }
                }
            ",
            "com/example/model/User.java" => r#"
                package com.example.model;
                public class User {
                    public String getEmail() {
                        return "";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }
}

lsp_test_suite! {
    name: "Method Navigation - Same File",

    test simple_method_call_same_file {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public void test() {
                        help<|>er();
                    }

                    private void helper() {}
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "Service.java"
    }

    test static_method_call_same_file {
        files: {
            "com/example/Utils.java" => r"
                package com.example;

                public class Utils {
                    public void test() {
                        Utils.helpe<|>r();
                    }

                    public static void helper() {}
                }
            "
        },
        nav: "com/example/Utils.java", "<|>" => "Utils.java"
    }

    test this_method_call_same_file {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public void test() {
                        this.proce<|>ss();
                    }

                    public void process() {}
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "Service.java"
    }

    test recursive_method_call {
        files: {
            "com/example/Math.java" => r"
                package com.example;

                public class Math {
                    public int factorial(int n) {
                        if (n <= 1) return 1;
                        return n * factori<|>al(n - 1);
                    }
                }
            "
        },
        nav: "com/example/Math.java", "<|>" => "Math.java"
    }

    test super_method_call {
        files: {
            "com/example/Child.java" => r"
                package com.example;

                public class Child extends Parent {
                    public void test() {
                        super.proce<|>ss();
                    }
                }

                class Parent {
                    public void process() {}
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Child.java"
    }

    test overloaded_method_call {
        files: {
            "com/example/Service.java" => r#"
                package com.example;

                public class Service {
                    public void test() {
                        proce<|>ss("test");
                    }

                    public void process() {}
                    public void process(String s) {}
                    public void process(int n) {}
                }
            "#
        },
        nav: "com/example/Service.java", "<|>" => "Service.java"
    }
}

lsp_test_suite! {
    name: "Method Navigation - Constructor Calls",

    test constructor_call_cross_file {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.User;

                public class Main {
                    public void test() {
                        User user = new Us<|>er();
                    }
                }
            ",
            "com/example/model/User.java" => r"
                package com.example.model;
                public class User {
                    public User() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "User.java"
    }

    test constructor_call_same_file {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public void test() {
                        Service s = new Servi<|>ce();
                    }

                    public Service() {}
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "Service.java"
    }

    test qualified_constructor_call {
        files: {
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    public void test() {
                        java.util.ArrayLi<|>st<String> list = new java.util.ArrayList<>();
                    }
                }
            ",
            "java/util/ArrayList.java" => r"
                package java.util;
                public class ArrayList {
                    public ArrayList() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "ArrayList.java"
    }
}

lsp_test_suite! {
    name: "Method Navigation - Edge Cases",

    test method_in_nested_class {
        files: {
            "com/example/Outer.java" => r"
                package com.example;

                public class Outer {
                    public void test() {
                        Inner inner = new Inner();
                        inner.proce<|>ss();
                    }

                    public class Inner {
                        public void process() {}
                    }
                }
            "
        },
        nav: "com/example/Outer.java", "<|>" => "Outer.java"
    }

    test method_in_anonymous_class {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public void test() {
                        Runnable r = new Runnable() {
                            public void run() {
                                help<|>er();
                            }
                        };
                    }

                    private void helper() {}
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "Service.java"
    }

    test method_in_lambda {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public void test() {
                        Runnable r = () -> help<|>er();
                    }

                    private void helper() {}
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "Service.java"
    }

    test generic_method_call {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                import com.example.model.Container;

                public class Main {
                    public void test() {
                        Container<String> c = new Container<>();
                        c.g<|>et();
                    }
                }
            ",
            "com/example/model/Container.java" => r"
                package com.example.model;
                public class Container<T> {
                    public T get() {
                        return null;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Container.java"
    }

    test varargs_method_call {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import com.example.Utils;

                public class Main {
                    public void test() {
                        Utils.forma<|>t("test", "arg1", "arg2");
                    }
                }
            "#,
            "com/example/Utils.java" => r#"
                package com.example;
                public class Utils {
                    public static String format(String template, String... args) {
                        return "";
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }

    test method_with_generic_type_parameter {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import com.example.Utils;

                public class Main {
                    public void test() {
                        Utils.execut<|>eWithLock(() -> {
                            return "result";
                        });
                    }
                }
            "#,
            "com/example/Utils.java" => r"
                package com.example;
                import java.util.function.Supplier;

                public class Utils {
                    public static <T> T executeWithLock(Supplier<T> supplier) {
                        return supplier.get();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Utils.java"
    }
}
