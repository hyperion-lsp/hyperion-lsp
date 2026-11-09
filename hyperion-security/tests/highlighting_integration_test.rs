// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Integration tests for syntax highlighting in CLI output

use hyperion_security::CodeHighlighter;
use std::path::PathBuf;

#[test]
fn test_java_highlighting_enabled() {
    let highlighter = CodeHighlighter::new().with_enabled(true);
    let code = r#"public class Test {
    public void method() {
        String s = "hello";
    }
}"#;

    let highlighted = highlighter.highlight(code, &PathBuf::from("Test.java"));

    // Should contain ANSI escape codes
    assert!(highlighted.contains("\x1b["));
    // Should be longer than original due to color codes
    assert!(highlighted.len() > code.len());
}

#[test]
fn test_kotlin_highlighting_enabled() {
    let highlighter = CodeHighlighter::new().with_enabled(true);
    let code = r#"class Test {
    fun method() {
        val s = "hello"
    }
}"#;

    let highlighted = highlighter.highlight(code, &PathBuf::from("Test.kt"));

    assert!(highlighted.contains("\x1b["));
    assert!(highlighted.len() > code.len());
}

#[test]
fn test_groovy_highlighting_enabled() {
    let highlighter = CodeHighlighter::new().with_enabled(true);
    let code = r#"class Test {
    def method() {
        def s = "hello"
    }
}"#;

    let highlighted = highlighter.highlight(code, &PathBuf::from("build.gradle"));

    assert!(highlighted.contains("\x1b["));
}

#[test]
fn test_highlighting_disabled() {
    let highlighter = CodeHighlighter::new().with_enabled(false);
    let code = r#"public class Test {
    public void method() {
        String s = "hello";
    }
}"#;

    let highlighted = highlighter.highlight(code, &PathBuf::from("Test.java"));

    // Should be identical to original
    assert_eq!(highlighted, code);
    // Should not contain ANSI codes
    assert!(!highlighted.contains("\x1b["));
}

#[test]
fn test_highlighting_preserves_content() {
    let highlighter = CodeHighlighter::new().with_enabled(true);
    let code = "public class Test {}";

    let highlighted = highlighter.highlight(code, &PathBuf::from("Test.java"));

    // Remove ANSI codes to verify content is preserved
    let stripped = strip_ansi_codes(&highlighted);
    assert_eq!(stripped.trim(), code);
}

#[test]
fn test_unknown_extension_fallback() {
    let highlighter = CodeHighlighter::new().with_enabled(true);
    let code = "some random code";

    // Should still work with unknown extension (uses plain text)
    let highlighted = highlighter.highlight(code, &PathBuf::from("unknown.xyz"));

    assert!(!highlighted.is_empty());
}

/// Strip ANSI escape codes from a string
fn strip_ansi_codes(s: &str) -> String {
    // Simple regex-free ANSI stripping
    let mut result = String::new();
    let mut in_escape = false;

    for ch in s.chars() {
        if ch == '\x1b' {
            in_escape = true;
        } else if in_escape && ch == 'm' {
            in_escape = false;
        } else if !in_escape {
            result.push(ch);
        }
    }

    result
}
