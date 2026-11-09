// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for workspace-based type hierarchy resolution.
//!
//! These tests verify that `is_subtype_in_workspace()` and `walk_hierarchy()`
//! correctly resolve type relationships using the workspace index.

use crate::lsp_test_framework::LspTest;

/// Helper to test subtype relationship via workspace
fn assert_subtype(
    context: &crate::lsp_test_framework::SharedLspContext,
    subtype: &str,
    supertype: &str,
) {
    assert!(
        hyperion_semantics::types::is_subtype_in_workspace(subtype, supertype, &context.index),
        "{subtype} should be a subtype of {supertype}"
    );
}

/// Helper to test non-subtype relationship via workspace
fn assert_not_subtype(
    context: &crate::lsp_test_framework::SharedLspContext,
    subtype: &str,
    supertype: &str,
) {
    assert!(
        !hyperion_semantics::types::is_subtype_in_workspace(subtype, supertype, &context.index),
        "{subtype} should NOT be a subtype of {supertype}"
    );
}

#[test]
fn test_basic_inheritance_via_workspace() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Parent.java",
        r"
        package com.example;

        public class Parent {
            public void parentMethod() {}
        }
    ",
    );

    test.add_file(
        "com/example/Child.java",
        r"
        package com.example;

        public class Child extends Parent {
            public void childMethod() {}
        }
    ",
    );

    let context = test.into_shared_context_with_options(false).unwrap();

    // Child extends Parent
    assert_subtype(&context, "com.example.Child", "com.example.Parent");

    // Parent does NOT extend Child (no reverse relationship)
    assert_not_subtype(&context, "com.example.Parent", "com.example.Child");

    // Self-subtype is always true
    assert_subtype(&context, "com.example.Child", "com.example.Child");
    assert_subtype(&context, "com.example.Parent", "com.example.Parent");
}

#[test]
fn test_multi_level_hierarchy() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/GrandParent.java",
        r"
        package com.example;

        public class GrandParent {}
    ",
    );

    test.add_file(
        "com/example/Parent.java",
        r"
        package com.example;

        public class Parent extends GrandParent {}
    ",
    );

    test.add_file(
        "com/example/Child.java",
        r"
        package com.example;

        public class Child extends Parent {}
    ",
    );

    let context = test.into_shared_context_with_options(false).unwrap();

    // Direct relationship
    assert_subtype(&context, "com.example.Child", "com.example.Parent");
    assert_subtype(&context, "com.example.Parent", "com.example.GrandParent");

    // Transitive relationship (grandchild → grandparent)
    assert_subtype(&context, "com.example.Child", "com.example.GrandParent");

    // No reverse relationships
    assert_not_subtype(&context, "com.example.GrandParent", "com.example.Child");
    assert_not_subtype(&context, "com.example.Parent", "com.example.Child");
}

#[test]
fn test_interface_implementation() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Printable.java",
        r"
        package com.example;

        public interface Printable {
            void print();
        }
    ",
    );

    test.add_file(
        "com/example/Document.java",
        r"
        package com.example;

        public class Document implements Printable {
            public void print() {}
        }
    ",
    );

    let context = test.into_shared_context_with_options(false).unwrap();

    // Document implements Printable
    assert_subtype(&context, "com.example.Document", "com.example.Printable");

    // No reverse relationship
    assert_not_subtype(&context, "com.example.Printable", "com.example.Document");
}

#[test]
fn test_interface_hierarchy() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/BaseInterface.java",
        r"
        package com.example;

        public interface BaseInterface {
            void baseMethod();
        }
    ",
    );

    test.add_file(
        "com/example/ExtendedInterface.java",
        r"
        package com.example;

        public interface ExtendedInterface extends BaseInterface {
            void extendedMethod();
        }
    ",
    );

    test.add_file(
        "com/example/Implementation.java",
        r"
        package com.example;

        public class Implementation implements ExtendedInterface {
            public void baseMethod() {}
            public void extendedMethod() {}
        }
    ",
    );

    let context = test.into_shared_context_with_options(false).unwrap();

    // ExtendedInterface extends BaseInterface
    assert_subtype(
        &context,
        "com.example.ExtendedInterface",
        "com.example.BaseInterface",
    );

    // Implementation implements ExtendedInterface (directly)
    assert_subtype(
        &context,
        "com.example.Implementation",
        "com.example.ExtendedInterface",
    );

    // Implementation is also a subtype of BaseInterface (transitive via interface hierarchy)
    assert_subtype(
        &context,
        "com.example.Implementation",
        "com.example.BaseInterface",
    );
}

#[test]
fn test_diamond_pattern() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Top.java",
        r"
        package com.example;

        public interface Top {}
    ",
    );

    test.add_file(
        "com/example/Left.java",
        r"
        package com.example;

        public interface Left extends Top {}
    ",
    );

    test.add_file(
        "com/example/Right.java",
        r"
        package com.example;

        public interface Right extends Top {}
    ",
    );

    test.add_file(
        "com/example/Bottom.java",
        r"
        package com.example;

        public class Bottom implements Left, Right {}
    ",
    );

    let context = test.into_shared_context_with_options(false).unwrap();

    // Bottom implements both Left and Right
    assert_subtype(&context, "com.example.Bottom", "com.example.Left");
    assert_subtype(&context, "com.example.Bottom", "com.example.Right");

    // Bottom is subtype of Top via both paths
    assert_subtype(&context, "com.example.Bottom", "com.example.Top");

    // Left and Right are both subtypes of Top
    assert_subtype(&context, "com.example.Left", "com.example.Top");
    assert_subtype(&context, "com.example.Right", "com.example.Top");

    // No reverse relationships
    assert_not_subtype(&context, "com.example.Top", "com.example.Bottom");
}

#[test]
fn test_cross_file_with_extends_and_implements() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/base/Entity.java",
        r"
        package com.example.base;

        public abstract class Entity {
            protected long id;
        }
    ",
    );

    test.add_file(
        "com/example/base/Auditable.java",
        r"
        package com.example.base;

        public interface Auditable {
            java.time.Instant getCreatedAt();
        }
    ",
    );

    test.add_file(
        "com/example/model/User.java",
        r"
        package com.example.model;

        import com.example.base.Entity;
        import com.example.base.Auditable;

        public class User extends Entity implements Auditable {
            private String name;
            public java.time.Instant getCreatedAt() { return null; }
        }
    ",
    );

    let context = test.into_shared_context_with_options(false).unwrap();

    // User extends Entity
    assert_subtype(
        &context,
        "com.example.model.User",
        "com.example.base.Entity",
    );

    // User implements Auditable
    assert_subtype(
        &context,
        "com.example.model.User",
        "com.example.base.Auditable",
    );

    // No relationship between Entity and Auditable
    assert_not_subtype(
        &context,
        "com.example.base.Entity",
        "com.example.base.Auditable",
    );
    assert_not_subtype(
        &context,
        "com.example.base.Auditable",
        "com.example.base.Entity",
    );
}

#[test]
fn test_simple_name_does_not_guess_supertype_identity() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Parent.java",
        r"
        package com.example;

        public class Parent {}
    ",
    );

    test.add_file(
        "com/example/Child.java",
        r"
        package com.example;

        public class Child extends Parent {}
    ",
    );

    let context = test.into_shared_context_with_options(false).unwrap();

    // Should work with FQN
    assert_subtype(&context, "com.example.Child", "com.example.Parent");

    // A simple name is not an exact nominal identity. Callers that have source
    // context must resolve it before asking the workspace hierarchy.
    assert_not_subtype(&context, "com.example.Child", "Parent");

    // The subtype identity is exact for the same reason.
    assert_not_subtype(&context, "Child", "com.example.Parent");
}

#[test]
fn test_object_is_universal_supertype() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/MyClass.java",
        r"
        package com.example;

        public class MyClass {}
    ",
    );

    let context = test.into_shared_context_with_options(false).unwrap();

    // Every reference type is a subtype of the exact Object identity. The
    // context-free helper must not guess what an unqualified spelling means.
    assert_subtype(&context, "com.example.MyClass", "java.lang.Object");
    assert_not_subtype(&context, "com.example.MyClass", "Object");

    // Object is not a subtype of MyClass
    assert_not_subtype(&context, "java.lang.Object", "com.example.MyClass");
}
