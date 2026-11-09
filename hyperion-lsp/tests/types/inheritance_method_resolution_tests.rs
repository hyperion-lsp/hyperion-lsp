// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Tests for method resolution with inheritance
// Testing: hover on inherited methods, var type inference with inherited methods
lsp_test_suite! {
    name: "Inheritance Method Resolution",

    // Test ArrayList.add() - ArrayList doesn't define add(), it's inherited from interface
    // The workspace finds the interface declaration first (which is correct Java semantics)
    test arraylist_inherited_add {
        files: {
            "com/example/Main.java" => r#"
                package com.example;
                import java.util.List;
                import java.util.ArrayList;
                public class Main {
                    public void test() {
                        List<String> list = new ArrayList<>();
                        list.a<|>dd("test");
                    }
                }
            "#,
            "java/util/List.java" => r"
                package java.util;
                public interface List<E> {
                    boolean add(E e);
                }
            ",
            "java/util/ArrayList.java" => r"
                package java.util;
                public class ArrayList<E> extends AbstractList<E> {
                    // ArrayList doesn't override add() - it's inherited
                }
            ",
            "java/util/AbstractList.java" => r"
                package java.util;
                public abstract class AbstractList<E> implements List<E> {
                    public boolean add(E e) {
                        return true;
                    }
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "List.java"
    }

    // Test method inherited from grandparent class
    test multi_level_method_call {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Child child = new Child();
                        child.grandParent<|>Method();
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
                public class Parent extends GrandParent {
                }
            ",
            "com/example/GrandParent.java" => r"
                package com.example;
                public class GrandParent {
                    public void grandParentMethod() {}
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "GrandParent.java"
    }

    // Test method inherited from interface
    test interface_method_inheritance {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        MyClass obj = new MyClass();
                        obj.interfaceMe<|>thod();
                    }
                }
            ",
            "com/example/MyClass.java" => r"
                package com.example;
                public class MyClass extends BaseClass implements MyInterface {
                }
            ",
            "com/example/BaseClass.java" => r"
                package com.example;
                public class BaseClass {
                }
            ",
            "com/example/MyInterface.java" => r"
                package com.example;
                public interface MyInterface {
                    void interfaceMethod();
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "MyInterface.java"
    }

    // Test method inherited from parent's implemented interface
    test parent_interface_method {
        files: {
            "com/example/Main.java" => r"
                package com.example;
                public class Main {
                    public void test() {
                        Child child = new Child();
                        child.interfaceMe<|>thod();
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
                public class Parent implements MyInterface {
                    public void interfaceMethod() {}
                }
            ",
            "com/example/MyInterface.java" => r"
                package com.example;
                public interface MyInterface {
                    void interfaceMethod();
                }
            "
        },
        nav: "com/example/Main.java", "<|>" => "Parent.java"
    }

    // Test unqualified inherited method call (direct parent) — no receiver, no `this.`
    test unqualified_inherited_method_direct_parent {
        files: {
            "com/example/AbstractValidator.java" => r"
                package com.example;
                public class AbstractValidator {
                    public void addFieldError(String field, String message) {}
                }
            ",
            "com/example/MyValidator.java" => r#"
                package com.example;
                public class MyValidator extends AbstractValidator {
                    public void validate() {
                        addField<|>Error("name", "Required");
                    }
                }
            "#
        },
        nav: "com/example/MyValidator.java", "<|>" => "AbstractValidator.java"
    }

    // Test unqualified inherited method call (grandparent)
    test unqualified_inherited_method_grandparent {
        files: {
            "com/example/GrandParent.java" => r"
                package com.example;
                public class GrandParent {
                    public void grandParentMethod() {}
                }
            ",
            "com/example/Parent.java" => r"
                package com.example;
                public class Parent extends GrandParent {
                }
            ",
            "com/example/Child.java" => r"
                package com.example;
                public class Child extends Parent {
                    public void doWork() {
                        grandParent<|>Method();
                    }
                }
            "
        },
        nav: "com/example/Child.java", "<|>" => "GrandParent.java"
    }

    // Test unqualified inherited method from interface default method
    test unqualified_inherited_interface_default_method {
        files: {
            "com/example/Validatable.java" => r"
                package com.example;
                public interface Validatable {
                    default void validate() {}
                }
            ",
            "com/example/MyClass.java" => r"
                package com.example;
                public class MyClass implements Validatable {
                    public void run() {
                        vali<|>date();
                    }
                }
            "
        },
        nav: "com/example/MyClass.java", "<|>" => "Validatable.java"
    }

    // Non-regression: unqualified method call in same class still works
    test unqualified_same_class_method {
        files: {
            "com/example/MyClass.java" => r"
                package com.example;
                public class MyClass {
                    public void helper() {}
                    public void doWork() {
                        hel<|>per();
                    }
                }
            "
        },
        nav: "com/example/MyClass.java", "<|>" => "MyClass.java"
    }

    // Non-regression: local variable still resolves to its declaration, not an inherited member
    test local_variable_not_confused_with_inherited_method {
        files: {
            "com/example/BaseService.java" => r"
                package com.example;
                public class BaseService {
                    public void count() {}
                }
            ",
            "com/example/MyService.java" => r"
                package com.example;
                public class MyService extends BaseService {
                    public void doWork() {
                        int count = 42;
                        int result = cou<|>nt + 1;
                    }
                }
            "
        },
        nav: "com/example/MyService.java", "<|>" => "MyService.java"
    }
}

goto_test! {
    unqualified_same_class_overload_uses_arity {
    file "com/example/OverloadDemo.java" r#"
        package com.example;

        public class OverloadDemo {
            public void «def_noarg»log() {}

            public void «def_string»log(String message) {}

            public void run() {
                lo«call»g("hello");
            }
        }
    "#;

    target "com/example/OverloadDemo.java" "call";

    expect "com/example/OverloadDemo.java" "def_string";
    }
}

goto_test! {
    unqualified_inherited_overload_uses_arity {
    file "com/example/Parent.java" r"
        package com.example;

        public class Parent {
            public void «def_noarg»overload() {}

            public void «def_string»overload(String value) {}
        }
    ";

    file "com/example/Child.java" r#"
        package com.example;

        public class Child extends Parent {
            public void run() {
                ov«call»erload("hello");
            }
        }
    "#;

    target "com/example/Child.java" "call";

    expect "com/example/Parent.java" "def_string";
    }
}

goto_test! {
    unqualified_same_class_overload_same_arity_uses_type {
    file "com/example/TypeOverloadDemo.java" r#"
        package com.example;

        public class TypeOverloadDemo {
            public void «def_integer»process(Integer value) {}

            public void «def_string»process(String value) {}

            public void run() {
                pro«call»cess("hello");
            }
        }
    "#;

    target "com/example/TypeOverloadDemo.java" "call";

    expect "com/example/TypeOverloadDemo.java" "def_string";
    }
}

goto_test! {
    unqualified_inherited_overload_same_arity_uses_type {
    file "com/example/TypeParent.java" r"
        package com.example;

        public class TypeParent {
            public void «def_integer»process(Integer value) {}

            public void «def_string»process(String value) {}
        }
    ";

    file "com/example/TypeChild.java" r#"
        package com.example;

        public class TypeChild extends TypeParent {
            public void run() {
                pro«call»cess("hello");
            }
        }
    "#;

    target "com/example/TypeChild.java" "call";

    expect "com/example/TypeParent.java" "def_string";
    }
}

goto_test! {
    overloaded_method_declaration_resolves_to_self {
    file "com/example/DeclOverloadDemo.java" r"
        package com.example;

        public class DeclOverloadDemo {
            public void ping() {}

            public void «decl_string»pin«decl_cursor»g(String value) {}
        }
    ";

    target "com/example/DeclOverloadDemo.java" "decl_cursor";

    expect "com/example/DeclOverloadDemo.java" "decl_string";
    }
}

// Hover tests for inherited methods
lsp_hover_test! {
    name: hover_inherited_method,
    files: {
        "com/example/Main.java" => r"
            package com.example;
            public class Main {
                public void test() {
                    Child child = new Child();
                    child.parentMe<|>thod();
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
                public void parentMethod() {}
            }
        "
    },
    hover: "com/example/Main.java", "<|>",
    contains: ["parentMethod"]
}

lsp_hover_test! {
    name: hover_multi_level_inherited_method,
    files: {
        "com/example/Main.java" => r"
            package com.example;
            public class Main {
                public void test() {
                    GrandChild obj = new GrandChild();
                    obj.grandParent<|>Method();
                }
            }
        ",
        "com/example/GrandChild.java" => r"
            package com.example;
            public class GrandChild extends Child {
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
    hover: "com/example/Main.java", "<|>",
    contains: ["grandParentMethod"]
}
