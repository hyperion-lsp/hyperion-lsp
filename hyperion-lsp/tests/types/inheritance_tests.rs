// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Comprehensive inheritance resolution tests
// Testing: extends, super calls, @Override, inherited methods, parent field access
lsp_test_suite! {
    name: "Inheritance Resolution",

    // Basic extends navigation
    test extends_clause_navigation {
        files: {
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Par<|>ent {
                    public void childMethod() {}
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public void parentMethod() {}
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }

    // super() constructor call
    test super_constructor_call {
        files: {
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    public Child() {
                        sup<|>er();
                    }
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public Parent() {}
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }

    // super() with arguments
    test super_constructor_with_args {
        files: {
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    public Child(String name) {
                        sup<|>er(name);
                    }
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public Parent(String name) {}
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }

    // super.method() call
    test super_method_call {
        files: {
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    public void process() {
                        super.pro<|>cess();
                    }
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public void process() {}
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }

    // super.field access
    test super_field_access {
        files: {
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    public void test() {
                        String value = super.na<|>me;
                    }
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    protected String name;
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }

    // Call inherited method on child instance
    test call_inherited_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Child child = new Child();
                        child.parentMeth<|>od();
                    }
                }
            ",
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    public void childMethod() {}
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public void parentMethod() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Parent.java"
    }

    // @Override annotation navigation
    test override_annotation {
        files: {
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    @Override
                    public void proce<|>ss() {}
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public void process() {}
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }

    // Multi-level inheritance
    test multi_level_inheritance {
        files: {
            "com/example/GrandChild.java" => r"
                package com.example;
                public class GrandChild extends Child {
                    public void test() {
                        this.grandParentMeth<|>od();
                    }
                }
            ",
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public void grandParentMethod() {}
                }
            "
        },
        nav: "com/example/GrandChild.java", "<|>" => "Parent.java"
    }

    // Access inherited protected field
    test inherited_protected_field {
        files: {
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    public void test() {
                        System.out.println(protectedFie<|>ld);
                    }
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    protected String protectedField;
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }

    // Inherited static method
    test inherited_static_method {
        files: {
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    public void test() {
                        staticHelp<|>er();
                    }
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent {
                    public static void staticHelper() {}
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "Parent.java"
    }
}
