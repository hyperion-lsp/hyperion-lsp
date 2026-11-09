// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Formatting must never create edits from parser-recovery trees.

use hyperion_fmt::{format_range, format_source, format_to_edits, FormatConfig, FormatError};

const MALFORMED_SOURCES: &[(&str, &str)] = &[
    ("raw_lf_in_string", "class C { String s = \"a\nb\"; }"),
    ("raw_cr_in_string", "class C { String s = \"a\rb\"; }"),
    ("raw_crlf_in_string", "class C { String s = \"a\r\nb\"; }"),
    ("raw_lf_in_char", "class C { char c = '\n'; }"),
    ("raw_cr_in_char", "class C { char c = '\r'; }"),
    ("oversized_octal_char", r"class C { char c = '\400'; }"),
    ("supplementary_char", "class C { char c = '😀'; }"),
    (
        "unterminated_class.java",
        include_str!("malformed_java/unterminated_class.java"),
    ),
    (
        "missing_parameter_type.java",
        include_str!("malformed_java/missing_parameter_type.java"),
    ),
    (
        "incomplete_expression.java",
        include_str!("malformed_java/incomplete_expression.java"),
    ),
    (
        "illegal_tokens.java",
        include_str!("malformed_java/illegal_tokens.java"),
    ),
];

#[test]
fn java_space_escapes_survive_formatting() {
    let source = r#"class C { char c='\s'; String s="a\sb"; }"#;
    let config = FormatConfig::default();
    let formatted = format_source(source, &config).expect("Java 15 space escapes are valid");
    assert!(formatted.contains(r"'\s'"), "{formatted}");
    assert!(formatted.contains(r#""a\sb""#), "{formatted}");
    assert_eq!(format_source(&formatted, &config).unwrap(), formatted);
}

#[test]
fn malformed_java_never_produces_formatting_edits() {
    let config = FormatConfig::default();

    for (name, source) in MALFORMED_SOURCES {
        let source_result = format_source(source, &config);
        assert!(
            matches!(source_result, Err(FormatError::ParseError(_))),
            "{name}: full-file formatting must stop at the parse boundary, got {source_result:?}"
        );

        let range_result = format_range(source, 0..source.len(), &config);
        assert!(
            matches!(range_result, Err(FormatError::ParseError(_))),
            "{name}: range formatting must stop at the parse boundary, got {range_result:?}"
        );

        let edits_result = format_to_edits(source, &config);
        assert!(
            matches!(edits_result, Err(FormatError::ParseError(_))),
            "{name}: invalid Java must not yield a partial edit, got {edits_result:?}"
        );
    }
}
