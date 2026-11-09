// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for method overload resolution
//
// This test suite verifies that goto-definition navigates to the correct
// method overload based on argument count.
lsp_test_suite! {
    name: "Method Overload Resolution",

    // Original bug scenario: recursive call to different overload
    test recursive_overload_call {
        files: {
            "com/example/Service.java" => r"
                package com.example;
                import java.util.Map;

                public class Service {
                    protected String getParam(String key, Map<String, String> values) {
                        return get<|>Param(key, values, true);
                    }

                    protected String getParam(String key, Map<String, String> values, boolean flag) {
                        return values.get(key);
                    }
                }
            "
        },
        nav: "com/example/Service.java", "<|>" => "Service.java"
    }

    // Simple overload with different argument counts - call with 2 args should go to 2-param overload
    test simple_overload_two_args {
        files: {
            "com/example/Test.java" => r#"
                package com.example;

                public class Test {
                    void process(String s) {}
                    void <target>process(String s, int n) {}

                    void test() {
                        proc<|>ess("a", 1);
                    }
                }
            "#
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // Call with 1 arg should go to 1-param overload
    test simple_overload_one_arg {
        files: {
            "com/example/Test.java" => r#"
                package com.example;

                public class Test {
                    void <target>process(String s) {}
                    void process(String s, int n) {}

                    void test() {
                        proc<|>ess("a");
                    }
                }
            "#
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }

    // Constructor overloads
    test constructor_overload_no_args {
        files: {
            "com/example/MyClass.java" => r"
                package com.example;

                public class MyClass {
                    public MyClass() {}
                    public MyClass(String name) {}
                    public MyClass(String name, int age) {}
                }
            ",
            "com/example/Main.java" => r"
                package com.example;

                public class Main {
                    void test() {
                        new MyC<|>lass();
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "MyClass.java"
    }

    test constructor_overload_two_args {
        files: {
            "com/example/MyClass.java" => r"
                package com.example;

                public class MyClass {
                    public MyClass() {}
                    public MyClass(String name) {}
                    public MyClass(String name, int age) {}
                }
            ",
            "com/example/Main.java" => r#"
                package com.example;

                public class Main {
                    void test() {
                        new MyC<|>lass("test", 25);
                    }
                }
            "#
        },
        nav: "com/example/Main.java", "<|>" => "MyClass.java"
    }

    // Inheritance: child class adds an overload, parent has another
    test inheritance_child_overload {
        files: {
            "com/example/Parent.java" => r"
                package com.example;

                public class Parent {
                    public void method(int n) {}
                }
            ",
            "com/example/Child.java" => r#"
                package com.example;

                public class Child extends Parent {
                    public void <target>method(String s) {}

                    public void test() {
                        meth<|>od("hello");
                    }
                }
            "#
        },
        nav: "com/example/Child.java", "<|>" => "<target>"
    }

    // Inheritance: call parent's overload from child
    // This test verifies type-based resolution: method(42) should go to Parent.method(int),
    // not Child.method(String), even though both have 1 parameter.
    test inheritance_parent_overload {
        files: {
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public void method(int n) {}
                }
            ",
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    public void method(String s) {}
                    public void test() {
                        meth<|>od(42);
                    }
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }

    // Cross-file overload: method on another class instance
    test cross_file_overload {
        files: {
            "com/example/Service.java" => r"
                package com.example;

                public class Service {
                    public void process(String s) {}
                    public void process(String s, int n) {}
                }
            ",
            "com/example/Client.java" => r#"
                package com.example;

                public class Client {
                    private Service service;

                    public void test() {
                        service.proc<|>ess("hello", 5);
                    }
                }
            "#
        },
        nav: "com/example/Client.java", "<|>" => "Service.java"
    }

    // Three overloads - pick the middle one
    test three_overloads_middle {
        files: {
            "com/example/Test.java" => r#"
                package com.example;

                public class Test {
                    void process() {}
                    void <target>process(String s) {}
                    void process(String s, int n) {}

                    void test() {
                        proc<|>ess("a");
                    }
                }
            "#
        },
        nav: "com/example/Test.java", "<|>" => "<target>"
    }
}
