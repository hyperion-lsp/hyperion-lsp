// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Regression tests for ternary expression navigation.

use crate::lsp_test_framework::LspTest;
use tower_lsp::lsp_types::GotoDefinitionResponse;

fn assert_goto_points_to_marker(
    test: &LspTest,
    file: &str,
    marker: &str,
    response: GotoDefinitionResponse,
) {
    let expected = test
        .get_marker_position(file, marker)
        .unwrap_or_else(|| panic!("Marker {marker} not found in {file}"));

    let actual = match response {
        GotoDefinitionResponse::Scalar(loc) => loc.range.start,
        GotoDefinitionResponse::Array(locs) => {
            locs.first()
                .expect("Empty goto-definition locations")
                .range
                .start
        }
        GotoDefinitionResponse::Link(links) => {
            links
                .first()
                .expect("Empty goto-definition links")
                .target_range
                .start
        }
    };

    assert_eq!(
        (actual.line, actual.character),
        (expected.line, expected.character),
        "Goto-definition location mismatch"
    );
}

#[test]
fn goto_definition_ternary_branch_identifier() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r#"
        package com.example;

        public class Test {
            public void run() {
                String <def_foo>foo = "foo";
                String bar = "bar";
                String value = true ? fo<|>o : bar;
            }
        }
        "#,
    );

    let result = test
        .goto_definition("com/example/Test.java", "<|>")
        .expect("goto_definition returned error")
        .expect("goto_definition returned None");

    assert_goto_points_to_marker(&test, "com/example/Test.java", "<def_foo>", result);
}

#[test]
fn goto_definition_ternary_receiver_method() {
    let mut test = LspTest::new();

    test.add_file(
        "com/example/Test.java",
        r"
        package com.example;

        public class Test {
            public void run() {
                Foo a = new Foo();
                Foo b = new Foo();
                boolean flag = true;
                (flag ? a : b).ge<|>tName();
            }
        }
        ",
    );

    test.add_file(
        "com/example/Foo.java",
        r#"
        package com.example;

        public class Foo {
            public String getName() { return "ok"; }
        }
        "#,
    );

    let result = test
        .goto_definition("com/example/Test.java", "<|>")
        .expect("goto_definition returned error")
        .expect("goto_definition returned None");

    match result {
        GotoDefinitionResponse::Scalar(loc) => {
            assert!(
                loc.uri.as_str().contains("Foo.java"),
                "Expected navigation to Foo.java, got {}",
                loc.uri
            );
        }
        GotoDefinitionResponse::Array(locs) => {
            assert!(!locs.is_empty(), "Expected locations from goto-definition");
            assert!(
                locs[0].uri.as_str().contains("Foo.java"),
                "Expected navigation to Foo.java, got {}",
                locs[0].uri
            );
        }
        GotoDefinitionResponse::Link(links) => {
            assert!(!links.is_empty(), "Expected links from goto-definition");
            assert!(
                links[0].target_uri.as_str().contains("Foo.java"),
                "Expected navigation to Foo.java, got {}",
                links[0].target_uri
            );
        }
    }
}
