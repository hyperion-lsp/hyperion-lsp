// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Test for cursor position bug on second goto-definition
//!
//! Reproduces the issue where doing goto-definition twice in a row
//! (without any file changes) results in incorrect cursor position on the second gd

use crate::lsp_test_framework::*;
use tower_lsp::lsp_types::*;

#[test]
fn test_second_gd_cursor_position() {
    let _ = env_logger::builder().is_test(true).try_init();

    let mut test = LspTest::new();

    test.add_file(
        "com/example/Foo.java",
        r"
        package com.example;

        import com.example.Bar;

        public class Foo {
            private Bar bar;

            public void test() {
                bar.ba<|>z();
            }
        }
    ",
    );

    test.add_file(
        "com/example/Bar.java",
        r#"
        package com.example;

        public class Bar {
            public void /*target:baz*/baz() {
                System.out.println("Hello");
            }
        }
    "#,
    );

    // First gd - should work
    let result1 = test.goto_definition("com/example/Foo.java", "<|>");
    let location1 = match &result1 {
        Ok(Some(GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains("Bar.java"),
                "First gd: Expected Bar.java, got: {}",
                loc.uri
            );
            println!(
                "✅ First gd: Navigated to {} at {:?}",
                loc.uri, loc.range.start
            );
            loc.clone()
        }
        Ok(Some(GotoDefinitionResponse::Array(locs))) => {
            assert!(!locs.is_empty(), "First gd: Empty location array");
            assert!(
                locs[0].uri.as_str().contains("Bar.java"),
                "First gd: Expected Bar.java, got: {}",
                locs[0].uri
            );
            println!(
                "✅ First gd: Navigated to {} at {:?}",
                locs[0].uri, locs[0].range.start
            );
            locs[0].clone()
        }
        _ => panic!("First gd failed: {result1:?}"),
    };

    // Second gd - should return EXACTLY the same position
    let result2 = test.goto_definition("com/example/Foo.java", "<|>");
    let location2 = match &result2 {
        Ok(Some(GotoDefinitionResponse::Scalar(loc))) => {
            assert!(
                loc.uri.as_str().contains("Bar.java"),
                "Second gd: Expected Bar.java, got: {}",
                loc.uri
            );
            println!(
                "✅ Second gd: Navigated to {} at {:?}",
                loc.uri, loc.range.start
            );
            loc.clone()
        }
        Ok(Some(GotoDefinitionResponse::Array(locs))) => {
            assert!(!locs.is_empty(), "Second gd: Empty location array");
            assert!(
                locs[0].uri.as_str().contains("Bar.java"),
                "Second gd: Expected Bar.java, got: {}",
                locs[0].uri
            );
            println!(
                "✅ Second gd: Navigated to {} at {:?}",
                locs[0].uri, locs[0].range.start
            );
            locs[0].clone()
        }
        _ => panic!("Second gd failed: {result2:?}"),
    };

    // CRITICAL: Verify both gd calls returned the EXACT same position
    assert_eq!(
        location1.range, location2.range,
        "❌ BUG FOUND: First gd returned {:?} but second gd returned {:?}",
        location1.range, location2.range
    );
    println!(
        "✅ Both goto-definitions returned identical positions: {:?}",
        location1.range
    );
}
