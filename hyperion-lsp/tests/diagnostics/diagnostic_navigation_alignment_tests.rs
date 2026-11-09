// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Diagnostic / Navigation Alignment Tests
//!
//! Verifies that the type checker (diagnostics) and goto-definition (navigation)
//! agree: if a type is resolvable for navigation, it should NOT produce a
//! false "unknown type" diagnostic, and vice versa.
//!
//! The `UnifiedTypeResolver` should have fixed historical divergence between
//! these paths; these tests verify that.

// ============================================================================
// Test 1: Imported class — goto-def works
// ============================================================================

lsp_goto_test! {
    name: imported_class_navigates,
    files: {
        "com/example/Main.java" => r"
            package com.example;

            import com.example.model.User;

            public class Main {
                public void test() {
                    Us<|>er user = new User();
                }
            }
        ",
        "com/example/model/User.java" => r"
            package com.example.model;

            public class User {
                private String name;
            }
        "
    },
    nav: "com/example/Main.java", "<|>" => "User.java"
}

// ============================================================================
// Test 2: Same-package class (no import) — goto-def works
// ============================================================================

lsp_goto_test! {
    name: same_package_class_navigates,
    files: {
        "com/example/Client.java" => r"
            package com.example;

            public class Client {
                public void test() {
                    Serv<|>ice svc = new Service();
                }
            }
        ",
        "com/example/Service.java" => r"
            package com.example;

            public class Service {
                public void handle() {}
            }
        "
    },
    nav: "com/example/Client.java", "<|>" => "Service.java"
}

// ============================================================================
// Test 3: Actually undefined symbol — goto-def returns None
// ============================================================================

lsp_goto_none_test! {
    name: undefined_symbol_no_navigation,
    files: {
        "com/example/Main.java" => r"
            package com.example;

            public class Main {
                public void test() {
                    Nonexist<|>ent x = null;
                }
            }
        "
    },
    nav: "com/example/Main.java", "<|>"
}

#[test]
fn unresolved_value_does_not_bind_to_sibling_type_member() {
    let mut test = crate::lsp_test_framework::LspTest::new();
    test.add_file(
        "com/example/Main.java",
        r"
            package com.example;

            class Unrelated {
                int value;
            }

            class Main {
                int read() {
                    return val<|>ue;
                }
            }
        ",
    );

    assert!(test
        .goto_definition("com/example/Main.java", "<|>")
        .expect("goto definition")
        .is_none());
    let hover = test
        .hover("com/example/Main.java", "<|>")
        .expect("hover")
        .expect("syntax-only hover remains available for an unresolved identifier");
    let hover_text = crate::lsp_test_framework::hover_to_text(&hover);
    assert!(!hover_text.contains("Unrelated"), "{hover_text}");
    assert!(!hover_text.contains("**Field**"), "{hover_text}");
    assert!(test
        .find_references("com/example/Main.java", "<|>", true)
        .expect("find references")
        .is_empty());
    assert!(test
        .prepare_rename("com/example/Main.java", "<|>")
        .expect_err("prepare rename must reject an unresolved sibling member")
        .contains("does not resolve to a declaration"));
}

lsp_goto_none_test! {
    name: inaccessible_qualified_field_has_no_navigation,
    files: {
        "com/example/Secret.java" => r"
            package com.example;
            public class Secret { private int value; }
        ",
        "com/example/Reader.java" => r"
            package com.example;
            public class Reader {
                int read(Secret secret) { return secret.val<|>ue; }
            }
        "
    },
    nav: "com/example/Reader.java", "<|>"
}

#[test]
fn inaccessible_field_is_rejected_consistently_across_symbol_features() {
    let mut test = crate::lsp_test_framework::LspTest::new();
    test.add_file(
        "com/example/Secret.java",
        "package com.example; public class Secret { private int value; }",
    );
    test.add_file(
        "com/example/Reader.java",
        concat!(
            "package com.example; public class Reader { ",
            "int read(Secret secret) { return secret.val<|>ue; } }"
        ),
    );

    assert!(test
        .goto_definition("com/example/Reader.java", "<|>")
        .expect("goto definition")
        .is_none());
    assert!(test
        .hover("com/example/Reader.java", "<|>")
        .expect("hover")
        .is_none());
    assert!(test
        .find_references("com/example/Reader.java", "<|>", true)
        .expect("find references")
        .is_empty());
    let prepare_error = test
        .prepare_rename("com/example/Reader.java", "<|>")
        .expect_err("prepare rename must reject an inaccessible binding");
    assert!(prepare_error.contains("does not resolve to a declaration"));
    let rename_error = test
        .rename("com/example/Reader.java", "<|>", "renamed")
        .expect_err("rename must reject an inaccessible binding");
    assert!(rename_error.contains("does not resolve to a declaration"));
}

lsp_goto_test! {
    name: package_qualified_field_navigates_in_same_package,
    files: {
        "com/example/Holder.java" => r"
            package com.example;
            public class Holder { int value; }
        ",
        "com/example/Reader.java" => r"
            package com.example;
            public class Reader {
                int read(Holder holder) { return holder.val<|>ue; }
            }
        "
    },
    nav: "com/example/Reader.java", "<|>" => "Holder.java"
}

lsp_goto_test! {
    name: private_qualified_field_navigates_for_nestmate,
    files: {
        "com/example/Outer.java" => r"
            package com.example;
            public class Outer {
                private int value;
                class Inner {
                    int read(Outer outer) { return outer.val<|>ue; }
                }
            }
        "
    },
    nav: "com/example/Outer.java", "<|>" => "Outer.java"
}

lsp_goto_test! {
    name: protected_qualified_field_navigates_through_subclass_receiver,
    files: {
        "base/Base.java" => r"
            package base;
            public class Base { protected int value; }
        ",
        "child/Child.java" => r"
            package child;
            public class Child extends base.Base {
                int read(Child child) { return child.val<|>ue; }
            }
        "
    },
    nav: "child/Child.java", "<|>" => "Base.java"
}

lsp_goto_none_test! {
    name: protected_qualified_field_rejects_base_receiver_across_packages,
    files: {
        "base/Base.java" => r"
            package base;
            public class Base { protected int value; }
        ",
        "child/Child.java" => r"
            package child;
            public class Child extends base.Base {
                int read(base.Base base) { return base.val<|>ue; }
            }
        "
    },
    nav: "child/Child.java", "<|>"
}

// ============================================================================
// Test 4: Inner class reference — goto-def works
// ============================================================================

lsp_goto_test! {
    name: inner_class_navigates,
    files: {
        "com/example/Outer.java" => r"
            package com.example;

            public class Outer {
                public static class Inner {
                    public void doWork() {}
                }
            }
        ",
        "com/example/Main.java" => r"
            package com.example;

            import com.example.Outer;

            public class Main {
                public void test() {
                    Outer.Inn<|>er inner = new Outer.Inner();
                }
            }
        "
    },
    nav: "com/example/Main.java", "<|>" => "Outer.java"
}

lsp_goto_test! {
    name: imported_enum_class_literal_navigates_to_its_type,
    files: {
        "Use.java" => "import model.SomeEnum; class Use { Object type() { return SomeEn<|>um.class; } }",
        "model/SomeEnum.java" => "package model; public enum SomeEnum { ONE }"
    },
    nav: "Use.java", "<|>" => "SomeEnum.java"
}
