// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests: goto definition on an @Override method declaration
//! name, including exact parameter-identity matching of the overridden
//! method.

use crate::lsp_test_framework::LspTest;

#[test]
fn goto_definition_on_plain_decl_name_self_navigates() {
    let mut test = LspTest::new();
    test.add_file(
        "com/example/Widget.java",
        r"package com.example;

public class Widget {

    private final String name;

    public Widget(String name) {
        this.name = name;
    }

    public String «decl»na<|>me() {
        return this.name;
    }
}

",
    );

    let resp = test
        .goto_definition("com/example/Widget.java", "<|>")
        .expect("request failed");

    let decl = test
        .get_marker_position("com/example/Widget.java", "«decl»")
        .unwrap();

    let locs = crate::lsp_test_framework::goto_response_to_locations(resp);
    assert_eq!(locs.len(), 1, "expected exactly one location");
    let loc = &locs[0];
    assert!(
        loc.uri.as_str().ends_with("Widget.java") && loc.range.start.line == decl.line,
        "expected self-navigation to the declaration line {}, got {} line {}",
        decl.line,
        loc.uri,
        loc.range.start.line
    );
}

#[test]
fn goto_definition_on_override_uses_exact_parameter_identity() {
    let mut test = LspTest::new();
    test.add_file("a/User.java", "package a; public class User {}\n");
    test.add_file("b/User.java", "package b; public class User {}\n");
    test.add_file(
        "com/example/Base.java",
        r"package com.example;
public class Base {
    public void «wrong»accept(a.User user) {}
    public void «right»accept(b.User user) {}
}
",
    );
    test.add_file(
        "com/example/Child.java",
        r"package com.example;
public class Child extends Base {
    @Override
    public void acc<|>ept(b.User user) {}
}
",
    );

    let response = test
        .goto_definition("com/example/Child.java", "<|>")
        .expect("request failed");
    let right = test
        .get_marker_position("com/example/Base.java", "«right»")
        .unwrap();
    let wrong = test
        .get_marker_position("com/example/Base.java", "«wrong»")
        .unwrap();
    let locations = crate::lsp_test_framework::goto_response_to_locations(response);
    assert_eq!(locations.len(), 1, "expected one exact overridden method");
    assert_eq!(locations[0].range.start.line, right.line);
    assert_ne!(locations[0].range.start.line, wrong.line);
}

#[test]
fn goto_definition_on_override_rejects_unindexed_parameter_change() {
    let mut test = LspTest::new();
    test.add_file("a/User.java", "package a; public class User {}\n");
    test.add_file("b/User.java", "package b; public class User {}\n");
    test.add_file(
        "com/example/Base.java",
        r"package com.example;
public class Base {
    public void accept(a.User user) {}
    public void accept(b.User user) {}
}
",
    );
    test.add_file(
        "com/example/Child.java",
        r"package com.example;
public class Child extends Base {
    @Override
    public void accept(a.User user) {}
}
",
    );
    let shared = test.into_shared_context_with_options(false).unwrap();

    let response = shared
        .goto_definition_with_unindexed_source(
            "com/example/Child.java",
            r"package com.example;
public class Child extends Base {
    @Override
    public void acc<|>ept(b.User user) {}
}
",
            "<|>",
        )
        .expect("request failed");
    let locations = crate::lsp_test_framework::goto_response_to_locations(response);
    assert!(
        locations
            .iter()
            .all(|location| !location.uri.as_str().ends_with("Base.java")),
        "stale indexed overload must not drive @Override navigation: {locations:?}"
    );
}
