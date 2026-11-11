// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Contracts for the shared LSP fixture marker parser and JDK cache.
//!
//! These live outside `lsp_test_framework.rs` so importing the framework from
//! another integration target does not re-run these contracts in every
//! target.

use crate::lsp_test_framework::parse_markers;

#[test]
fn cursor_marker_is_removed_and_records_its_position() {
    let (clean, markers) = parse_markers("foo<|>bar");

    assert_eq!(clean, "foobar");
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "<|>");
    assert_eq!(markers[0].character, 3);
}

#[test]
fn comment_marker_is_removed_and_named() {
    let (clean, markers) = parse_markers("foo /*target:bar*/ baz");

    assert_eq!(clean, "foo  baz");
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "target:bar");
}

#[test]
fn caret_marker_points_to_the_previous_line() {
    let content = r"
            bar.baz();
            ^
        ";
    let (_clean, markers) = parse_markers(content);

    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "^");
    assert_eq!(markers[0].line, 1);
}

#[test]
fn multiple_marker_styles_are_collected() {
    let content = r"
            Foo f = new /*target:Foo*/Foo();
            f.bar<|>();
        ";
    let (_clean, markers) = parse_markers(content);

    assert_eq!(markers.len(), 2);
    assert!(markers.iter().any(|marker| marker.name == "target:Foo"));
    assert!(markers.iter().any(|marker| marker.name == "<|>"));
}

/// Exercise cold, memory, and disk cache paths in isolated processes. The
/// first workspace deliberately shadows a JDK type, as many language-feature
/// fixtures do; neither cache may retain that workspace's elected symbols.
#[test]
fn jdk_cache_is_independent_of_source_fixtures() {
    const PHASE: &str = "HYPERION_JDK_CACHE_CONTRACT_PHASE";
    if let Ok(phase) = std::env::var(PHASE) {
        if phase == "seed" {
            let mut fixture = crate::lsp_test_framework::LspTest::new();
            fixture.add_file(
                "java/lang/String.java",
                "package java.lang; public final class String { public int fixtureOnly() { return 1; } }",
            );
            fixture.add_file(
                "Use.java",
                "class Use { void run(String value) { value.<|> } }",
            );
            let items = fixture
                .completion("Use.java", "<|>")
                .expect("fixture completion");
            assert!(items.iter().any(|item| item.label == "fixtureOnly"));
            assert!(!items.iter().any(|item| item.label == "substring"));
        }

        let mut real = crate::lsp_test_framework::LspTest::new();
        real.add_file(
            "Use.java",
            "class Use { void run(String value) { value.<|> } }",
        );
        let items = real.completion("Use.java", "<|>").expect("JDK completion");
        assert!(
            items.iter().any(|item| item.label == "substring"),
            "JDK String must survive the {phase} cache path"
        );
        assert!(!items.iter().any(|item| item.label == "fixtureOnly"));
        return;
    }

    let cache = tempfile::tempdir().expect("isolated JDK cache");
    for phase in ["seed", "disk"] {
        let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "lsp_test_framework_contract_tests::jdk_cache_is_independent_of_source_fixtures",
                "--nocapture",
            ])
            .env(PHASE, phase)
            .env("HYPERION_TEST_CACHE_DIR", cache.path())
            .output()
            .expect("run isolated cache contract");
        assert!(
            output.status.success(),
            "{phase} cache contract failed:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        let expected_hit = if phase == "seed" {
            "TEST JDK memory cache HIT"
        } else {
            "TEST JDK cache HIT"
        };
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected_hit),
            "{phase} must exercise its cache hit, rather than reindexing the JDK",
        );
    }
}
