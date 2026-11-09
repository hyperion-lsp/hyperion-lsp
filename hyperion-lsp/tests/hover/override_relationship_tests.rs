// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Hover's "Overrides:" relationship.
//!
//! Hovering an `@Override` method declaration must name the exact declaration
//! it overrides. Two things have to be right for that: the enclosing type has
//! to be the one the cursor is in (not a same-named class in another package),
//! and the search for the overridden declaration has to walk the whole
//! hierarchy, since the nearest declaration may sit on a grandparent or on a
//! superinterface the class never names (JLS 8.4.8, 9.4.1).

use crate::lsp_test_framework::{hover_to_text, LspTest};

fn hover_text(test: &mut LspTest, file: &str) -> String {
    test.hover(file, "<|>")
        .unwrap_or_else(|error| panic!("hover request failed for {file}: {error}"))
        .map(|hover| hover_to_text(&hover))
        .unwrap_or_default()
}

fn assert_overrides(text: &str, expected_owner: &str, context: &str) {
    assert!(
        text.contains("**Overrides:**"),
        "{context}: hover has no override relationship at all:\n{text}"
    );
    assert!(
        text.contains(expected_owner),
        "{context}: expected the override target to be `{expected_owner}`:\n{text}"
    );
}

#[test]
fn an_override_of_a_direct_superclass_method_names_it() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Parent.java",
        "package com.example;\npublic class Parent {\n    public void run() {}\n}\n",
    );
    test.add_file(
        "com/example/Child.java",
        "package com.example;\npublic class Child extends Parent {\n    @Override\n    public void r<|>un() {}\n}\n",
    );

    let text = hover_text(&mut test, "com/example/Child.java");
    assert_overrides(&text, "com.example.Parent.run", "direct superclass");
}

/// JLS 8.4.8: the overridden declaration may sit on a grandparent.
#[test]
fn an_override_of_a_grandparent_method_names_it() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Root.java",
        "package com.example;\npublic class Root {\n    public void tick() {}\n}\n",
    );
    test.add_file(
        "com/example/Middle.java",
        "package com.example;\npublic class Middle extends Root {\n}\n",
    );
    test.add_file(
        "com/example/Leaf.java",
        "package com.example;\npublic class Leaf extends Middle {\n    @Override\n    public void ti<|>ck() {}\n}\n",
    );

    let text = hover_text(&mut test, "com/example/Leaf.java");
    assert_overrides(&text, "com.example.Root.tick", "grandparent");
}

/// JLS 9.4.1: the implemented declaration may sit on a superinterface the
/// class never names.
#[test]
fn an_implementation_of_a_superinterface_method_names_it() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Base.java",
        "package com.example;\npublic interface Base {\n    void serve();\n}\n",
    );
    test.add_file(
        "com/example/Derived.java",
        "package com.example;\npublic interface Derived extends Base {\n}\n",
    );
    test.add_file(
        "com/example/Impl.java",
        "package com.example;\npublic class Impl implements Derived {\n    @Override\n    public void se<|>rve() {}\n}\n",
    );

    let text = hover_text(&mut test, "com/example/Impl.java");
    assert_overrides(&text, "com.example.Base.serve", "transitive superinterface");
}

/// The enclosing type must come from the cursor, not from a workspace-wide
/// simple-name lookup: two packages may both declare a `Parent`, and only one
/// of them is this class's supertype.
#[test]
fn a_same_named_class_in_another_package_is_not_the_override_target() {
    let mut test = LspTest::new();
    test.add_file(
        "left/Parent.java",
        "package left;\npublic class Parent {\n    public void act() {}\n}\n",
    );
    test.add_file(
        "right/Parent.java",
        "package right;\npublic class Parent {\n    public void act() {}\n}\n",
    );
    test.add_file(
        "app/Child.java",
        "package app;\n\nimport right.Parent;\n\npublic class Child extends Parent {\n    @Override\n    public void a<|>ct() {}\n}\n",
    );

    let text = hover_text(&mut test, "app/Child.java");
    assert_overrides(&text, "right.Parent.act", "imported supertype");
    assert!(
        !text.contains("left.Parent.act"),
        "hover attributed the override to a same-named class in another package:\n{text}"
    );
}

/// An enum constant body is not the only place an enum overrides something:
/// the enum itself may implement an interface.
#[test]
fn an_enum_implementation_names_its_interface_method() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Named.java",
        "package com.example;\npublic interface Named {\n    String label();\n}\n",
    );
    test.add_file(
        "com/example/Color.java",
        "package com.example;\npublic enum Color implements Named {\n    RED;\n\n    @Override\n    public String la<|>bel() {\n        return \"red\";\n    }\n}\n",
    );

    let text = hover_text(&mut test, "com/example/Color.java");
    assert_overrides(&text, "com.example.Named.label", "enum");
}

/// A record may implement an interface too.
#[test]
fn a_record_implementation_names_its_interface_method() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Sized.java",
        "package com.example;\npublic interface Sized {\n    int size();\n}\n",
    );
    test.add_file(
        "com/example/Pair.java",
        "package com.example;\npublic record Pair(int a, int b) implements Sized {\n    @Override\n    public int si<|>ze() {\n        return 2;\n    }\n}\n",
    );

    let text = hover_text(&mut test, "com/example/Pair.java");
    assert_overrides(&text, "com.example.Sized.size", "record");
}
