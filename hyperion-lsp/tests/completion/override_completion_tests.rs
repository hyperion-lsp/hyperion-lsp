// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Override and Implement Method Completion Tests
//!
//! Tests for override/implement method completion suggestions.

// Test 1: Override method from parent class
lsp_completion_test! {
    name: test_override_parent_method,
    files: {
        "com/example/Parent.java" => r#"
            package com.example;
            public class Parent {
                public String getName() {
                    return "parent";
                }
                public int getAge() {
                    return 0;
                }
            }
        "#,
        "com/example/Child.java" => r"
            package com.example;
            public class Child extends Parent {
                <|>
            }
        "
    },
    complete: "com/example/Child.java", "<|>",
    contains: ["Override getName (com.example.Parent)", "Override getAge (com.example.Parent)"]
}

// Test 2: Implement interface method
lsp_completion_test! {
    name: test_implement_interface_method,
    files: {
        "com/example/Runnable.java" => r"
            package com.example;
            public interface Runnable {
                void run();
            }
        ",
        "com/example/Task.java" => r"
            package com.example;
            public class Task implements Runnable {
                <|>
            }
        "
    },
    complete: "com/example/Task.java", "<|>",
    contains: ["Implement run (com.example.Runnable)"]
}

// Test 3: Multiple interfaces
lsp_completion_test! {
    name: test_implement_multiple_interfaces,
    files: {
        "com/example/A.java" => r"
            package com.example;
            public interface A {
                void methodA();
            }
        ",
        "com/example/B.java" => r"
            package com.example;
            public interface B {
                void methodB();
            }
        ",
        "com/example/Impl.java" => r"
            package com.example;
            public class Impl implements A, B {
                <|>
            }
        "
    },
    complete: "com/example/Impl.java", "<|>",
    contains: ["Implement methodA (com.example.A)", "Implement methodB (com.example.B)"]
}

// Test 4: Already overridden methods should not appear
lsp_completion_test! {
    name: test_filter_already_overridden,
    files: {
        "com/example/Parent.java" => r#"
            package com.example;
            public class Parent {
                public String getName() {
                    return "parent";
                }
                public int getAge() {
                    return 0;
                }
            }
        "#,
        "com/example/Child.java" => r#"
            package com.example;
            public class Child extends Parent {
                @Override
                public String getName() {
                    return "child";
                }
                <|>
            }
        "#
    },
    complete: "com/example/Child.java", "<|>",
    // getName should not appear since it's already overridden
    // Only getAge should be suggested
    contains: ["Override getAge (com.example.Parent)"]
}

// Test 5: Trigger on @Override annotation
lsp_completion_test! {
    name: test_trigger_on_override_annotation,
    files: {
        "com/example/Parent.java" => r#"
            package com.example;
            public class Parent {
                public String toString() {
                    return "parent";
                }
            }
        "#,
        "com/example/Child.java" => r"
            package com.example;
            public class Child extends Parent {
                @<|>
            }
        "
    },
    complete: "com/example/Child.java", "<|>",
    contains: ["Override toString (com.example.Parent)"]
}

// Test 6: No suggestions when no parent/interfaces
lsp_completion_test! {
    name: test_no_suggestions_without_inheritance,
    files: {
        "com/example/Standalone.java" => r"
            package com.example;
            public class Standalone {
                <|>
            }
        "
    },
    complete: "com/example/Standalone.java", "<|>"
    // Should not contain any Override/Implement suggestions
    // (but may contain other general completions)
    // Note: We can't test negative assertions with this macro
}

// Test 7: Both override and implement together
lsp_completion_test! {
    name: test_override_and_implement_together,
    files: {
        "com/example/Base.java" => r"
            package com.example;
            public class Base {
                public void baseMethod() {}
            }
        ",
        "com/example/Service.java" => r"
            package com.example;
            public interface Service {
                void serve();
            }
        ",
        "com/example/Impl.java" => r"
            package com.example;
            public class Impl extends Base implements Service {
                <|>
            }
        "
    },
    complete: "com/example/Impl.java", "<|>",
    contains: ["Override baseMethod (com.example.Base)", "Implement serve (com.example.Service)"]
}

// Test 8: Methods with parameters
lsp_completion_test! {
    name: test_override_method_with_parameters,
    files: {
        "com/example/Parent.java" => r"
            package com.example;
            public class Parent {
                public boolean equals(Object obj) {
                    return false;
                }
            }
        ",
        "com/example/Child.java" => r"
            package com.example;
            public class Child extends Parent {
                <|>
            }
        "
    },
    complete: "com/example/Child.java", "<|>",
    contains: ["Override equals (com.example.Parent)"]
}

lsp_completion_test! {
    name: explicit_supertype_import_wins_over_global_duplicate,
    files: {
        "left/Service.java" => r"
            package left;
            public interface Service { void wrong(); }
        ",
        "right/Service.java" => r"
            package right;
            public interface Service { void serve(); }
        ",
        "app/Impl.java" => r"
            package app;
            import right.Service;
            public class Impl implements Service {
                <|>
            }
        "
    },
    complete: "app/Impl.java", "<|>",
    contains: ["Implement serve (right.Service)"],
    not_contains: ["Implement wrong (left.Service)"]
}

// The candidate set is the type's *effective* method set, so it is
// transitive through superclasses and superinterfaces alike (JLS 8.4.8,
// 9.4.1). Reading the members of the directly written supertypes -- what this
// generator used to do -- never reaches either.
lsp_completion_test! {
    name: grandparent_methods_are_offered_for_override,
    files: {
        "com/example/Grand.java" => r"
            package com.example;
            public class Grand {
                public void fromGrand() {}
            }
        ",
        "com/example/Middle.java" => r"
            package com.example;
            public class Middle extends Grand {
                public void fromMiddle() {}
            }
        ",
        "com/example/Leaf.java" => r"
            package com.example;
            public class Leaf extends Middle {
                <|>
            }
        "
    },
    complete: "com/example/Leaf.java", "<|>",
    contains: [
        "Override fromMiddle (com.example.Middle)",
        "Override fromGrand (com.example.Grand)"
    ]
}

lsp_completion_test! {
    name: superinterface_methods_are_offered_for_implementation,
    files: {
        "com/example/Base.java" => r"
            package com.example;
            public interface Base {
                void base();
            }
        ",
        "com/example/Derived.java" => r"
            package com.example;
            public interface Derived extends Base {
                void derived();
            }
        ",
        "com/example/Impl2.java" => r"
            package com.example;
            public class Impl2 implements Derived {
                <|>
            }
        "
    },
    complete: "com/example/Impl2.java", "<|>",
    contains: [
        "Implement derived (com.example.Derived)",
        "Implement base (com.example.Base)"
    ]
}

// An interface `default` method already has a body, so it is overridable
// rather than owed. JLS 15.12.1 also forbids reaching it with plain `super`.
lsp_completion_test! {
    name: an_inherited_default_method_is_offered_as_an_override,
    files: {
        "com/example/Greeter.java" => r"
            package com.example;
            public interface Greeter {
                void required();
                default void greet() {}
            }
        ",
        "com/example/Polite.java" => r"
            package com.example;
            public class Polite implements Greeter {
                <|>
            }
        "
    },
    complete: "com/example/Polite.java", "<|>",
    contains: [
        "Implement required (com.example.Greeter)",
        "Override greet (com.example.Greeter)"
    ],
    not_contains: ["Implement greet (com.example.Greeter)"]
}
