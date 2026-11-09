// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for generic type argument completion
//!
//! Verifies that completion inside angle brackets (List<X>, Map<K, V>)
//! shows types and wildcard options.

use crate::lsp_test_framework::LspTest;

// Test 1: Second type argument in Map - wildcards should be offered
// This is the clearest case that works with the marker system
lsp_completion_test! {
    name: map_second_type_argument_wildcards,
    files: {
        "Test.java" => r"
            import java.util.Map;
            class Test {
                Map<String, <cur>> map;
            }
        "
    },
    complete: "Test.java", "<cur>",
    contains: ["?", "? extends", "? super"]
}

// Test 2: Basic generic - use comment marker to avoid angle bracket conflict
#[test]
fn list_generic_shows_wildcards() {
    let mut test = LspTest::new();
    // Use <cur> marker with space - otherwise <<cur>> parsing fails
    test.add_file("Test.java", "class Test { java.util.List< <cur>> list; }");
    test.add_file("User.java", "class User {}");

    let result = test.completion("Test.java", "<cur>");
    assert!(result.is_ok(), "Completion failed: {:?}", result.err());
    let completions = result.unwrap();

    // Should have wildcard
    let has_wildcard = completions.iter().any(|c| c.label == "?");
    assert!(
        has_wildcard,
        "Should offer '?' wildcard. Got: {:?}",
        completions.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
}

// Test 3: After comma - should show wildcards and types
#[test]
fn after_comma_shows_wildcards() {
    let mut test = LspTest::new();
    test.add_file(
        "Test.java",
        r"
        import java.util.Map;
        class Test {
            Map<String, <|>
        ",
    );

    let result = test.completion("Test.java", "<|>");
    assert!(result.is_ok(), "Completion failed: {:?}", result.err());
    let completions = result.unwrap();

    let has_wildcard_extends = completions.iter().any(|c| c.label == "? extends");
    assert!(
        has_wildcard_extends,
        "Should offer '? extends'. Got: {:?}",
        completions.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
}

// Test 4: Comparison operator should NOT trigger generic completion
#[test]
fn comparison_not_generic() {
    let mut test = LspTest::new();
    test.add_file(
        "Test.java",
        r"
        class Test {
            void foo() {
                int x = 5;
                if (x <
                     ^
    ",
    );

    let result = test.completion("Test.java", "^");
    assert!(result.is_ok(), "Completion failed: {:?}", result.err());
    let completions = result.unwrap();

    // Should NOT have wildcard completions (this is a comparison, not generic)
    let has_wildcard_extends = completions.iter().any(|c| c.label == "? extends");
    let has_wildcard_super = completions.iter().any(|c| c.label == "? super");

    assert!(
        !has_wildcard_extends && !has_wildcard_super,
        "Should not show wildcard completions in comparison context. Got: {:?}",
        completions.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
}

// Test 5: Generic context should show project types when typing prefix
#[test]
fn generic_shows_project_types() {
    let mut test = LspTest::new();
    // Type "My" prefix to trigger workspace search (requires 2+ chars)
    // Use caret marker positioned after "My"
    test.add_file(
        "Test.java",
        r"
import java.util.List;
class Test {
    List<My
           ^
",
    );
    test.add_file("MyClass.java", "class MyClass {}");

    let result = test.completion("Test.java", "^");
    assert!(result.is_ok(), "Completion failed: {:?}", result.err());
    let completions = result.unwrap();

    let has_my_class = completions.iter().any(|c| c.label == "MyClass");
    assert!(
        has_my_class,
        "Should show project class 'MyClass'. Got: {:?}",
        completions.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
}
