// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! The PBKDF2 iteration check reads the constructor's iteration count, not a digit
//! from the class name or another argument.

use hyperion_security::{ScanConfig, SecurityScanner};
use std::path::Path;

fn weak_encoder_messages(source: &str) -> Vec<String> {
    let scanner = SecurityScanner::new(ScanConfig::default());
    scanner
        .scan_source(source, Path::new("Encoders.java"))
        .into_iter()
        .map(|finding| finding.message)
        .filter(|message| message.contains("Pbkdf2PasswordEncoder configured"))
        .collect()
}

fn java(constructor_call: &str) -> String {
    format!(
        "class Encoders {{\n    Object make() {{\n        return {constructor_call};\n    }}\n}}\n"
    )
}

#[test]
fn low_iteration_count_is_reported() {
    let messages = weak_encoder_messages(&java(
        "new Pbkdf2PasswordEncoder(\"secret\", 16, 1000, 256)",
    ));
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("only 1000 iterations"), "{messages:?}");
}

#[test]
fn low_iteration_count_with_an_algorithm_is_reported() {
    let messages = weak_encoder_messages(&java(
        "new Pbkdf2PasswordEncoder(\"secret\", 16, 1000, Pbkdf2PasswordEncoder.SecretKeyFactoryAlgorithm.PBKDF2WithHmacSHA256)",
    ));
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("only 1000 iterations"), "{messages:?}");
}

#[test]
fn strong_iteration_count_is_not_reported() {
    assert!(
        weak_encoder_messages(&java("new Pbkdf2PasswordEncoder(\"\", 16, 310000, 256)")).is_empty()
    );
}

#[test]
fn low_iteration_count_in_the_spring_5_three_argument_form_is_reported() {
    let messages = weak_encoder_messages(&java("new Pbkdf2PasswordEncoder(\"secret\", 1000, 256)"));
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("only 1000 iterations"), "{messages:?}");
}

#[test]
fn digits_in_the_class_name_and_salt_are_not_the_count() {
    assert!(weak_encoder_messages(&java(
        "new Pbkdf2PasswordEncoder(\"salt123\", 16, 310_000, 256)"
    ))
    .is_empty());
}

#[test]
fn commas_and_parentheses_inside_a_string_or_char_secret_do_not_split_arguments() {
    for secret in [
        r#""a,b""#,
        r#""a)b""#,
        r#""a\",b""#,
        r#"String.valueOf(',')"#,
        r#"String.valueOf(')')"#,
        r#"String.valueOf('\'')"#,
    ] {
        let call = format!("new Pbkdf2PasswordEncoder({secret}, 16, 1000, 256)");
        let messages = weak_encoder_messages(&java(&call));
        assert_eq!(messages.len(), 1, "{call}: {messages:?}");
        assert!(messages[0].contains("only 1000 iterations"), "{messages:?}");
    }
}

#[test]
fn an_escaped_backslash_ends_the_secret_string() {
    let messages =
        weak_encoder_messages(&java(r#"new Pbkdf2PasswordEncoder("a\\", 16, 1000, 256)"#));
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("only 1000 iterations"), "{messages:?}");
}
