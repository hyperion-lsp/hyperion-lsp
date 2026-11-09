// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive tests for the Java lexer
//!
//! This test suite covers:
//! - Basic tokenization
//! - Keywords (all Java versions)
//! - Literals (integers, floats, strings, characters)
//! - Operators and punctuation
//! - Comments (line, block, javadoc)
//! - Whitespace handling
//! - Edge cases and error recovery

use hyperion_parser::lexer::Lexer;
use hyperion_parser::syntax::SyntaxKind;
use rowan::TextRange;

/// Helper function to collect tokens without trivia
fn lex_without_trivia(source: &str) -> Vec<(SyntaxKind, String)> {
    Lexer::new(source)
        .skip_trivia()
        .map(|(kind, text, _)| (kind, text.to_string()))
        .collect()
}

/// Helper function to collect all tokens including trivia
fn lex_all(source: &str) -> Vec<(SyntaxKind, String, TextRange)> {
    Lexer::new(source)
        .map(|(kind, text, range)| (kind, text.to_string(), range))
        .collect()
}

// ============================================================================
// Basic Tokenization Tests
// ============================================================================

#[test]
fn test_empty_source() {
    let source = "";
    let tokens = lex_all(source);
    assert_eq!(tokens.len(), 0);
}

#[test]
fn test_whitespace_only() {
    let source = "   \t\n\r\n  ";
    let tokens = lex_all(source);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, SyntaxKind::WHITESPACE);
}

#[test]
fn test_simple_class_declaration() {
    let source = "class Foo { }";
    let tokens = lex_without_trivia(source);

    assert_eq!(tokens.len(), 4);
    assert_eq!(tokens[0], (SyntaxKind::CLASS_KW, "class".to_string()));
    assert_eq!(tokens[1], (SyntaxKind::IDENT, "Foo".to_string()));
    assert_eq!(tokens[2], (SyntaxKind::L_BRACE, "{".to_string()));
    assert_eq!(tokens[3], (SyntaxKind::R_BRACE, "}".to_string()));
}

// ============================================================================
// Keyword Tests
// ============================================================================

#[test]
fn test_all_java_keywords() {
    let keywords = vec![
        ("abstract", SyntaxKind::ABSTRACT_KW),
        ("assert", SyntaxKind::ASSERT_KW),
        ("boolean", SyntaxKind::BOOLEAN_KW),
        ("break", SyntaxKind::BREAK_KW),
        ("byte", SyntaxKind::BYTE_KW),
        ("case", SyntaxKind::CASE_KW),
        ("catch", SyntaxKind::CATCH_KW),
        ("char", SyntaxKind::CHAR_KW),
        ("class", SyntaxKind::CLASS_KW),
        ("const", SyntaxKind::CONST_KW),
        ("continue", SyntaxKind::CONTINUE_KW),
        ("default", SyntaxKind::DEFAULT_KW),
        ("do", SyntaxKind::DO_KW),
        ("double", SyntaxKind::DOUBLE_KW),
        ("else", SyntaxKind::ELSE_KW),
        ("enum", SyntaxKind::ENUM_KW),
        ("extends", SyntaxKind::EXTENDS_KW),
        ("final", SyntaxKind::FINAL_KW),
        ("finally", SyntaxKind::FINALLY_KW),
        ("float", SyntaxKind::FLOAT_KW),
        ("for", SyntaxKind::FOR_KW),
        ("goto", SyntaxKind::GOTO_KW),
        ("if", SyntaxKind::IF_KW),
        ("implements", SyntaxKind::IMPLEMENTS_KW),
        ("import", SyntaxKind::IMPORT_KW),
        ("instanceof", SyntaxKind::INSTANCEOF_KW),
        ("int", SyntaxKind::INT_KW),
        ("interface", SyntaxKind::INTERFACE_KW),
        ("long", SyntaxKind::LONG_KW),
        ("native", SyntaxKind::NATIVE_KW),
        ("new", SyntaxKind::NEW_KW),
        ("package", SyntaxKind::PACKAGE_KW),
        ("private", SyntaxKind::PRIVATE_KW),
        ("protected", SyntaxKind::PROTECTED_KW),
        ("public", SyntaxKind::PUBLIC_KW),
        ("return", SyntaxKind::RETURN_KW),
        ("short", SyntaxKind::SHORT_KW),
        ("static", SyntaxKind::STATIC_KW),
        ("strictfp", SyntaxKind::STRICTFP_KW),
        ("super", SyntaxKind::SUPER_KW),
        ("switch", SyntaxKind::SWITCH_KW),
        ("synchronized", SyntaxKind::SYNCHRONIZED_KW),
        ("this", SyntaxKind::THIS_KW),
        ("throw", SyntaxKind::THROW_KW),
        ("throws", SyntaxKind::THROWS_KW),
        ("transient", SyntaxKind::TRANSIENT_KW),
        ("try", SyntaxKind::TRY_KW),
        ("void", SyntaxKind::VOID_KW),
        ("volatile", SyntaxKind::VOLATILE_KW),
        ("while", SyntaxKind::WHILE_KW),
    ];

    for (keyword, expected_kind) in keywords {
        let tokens = lex_without_trivia(keyword);
        assert_eq!(tokens.len(), 1, "Failed for keyword: {}", keyword);
        assert_eq!(
            tokens[0].0, expected_kind,
            "Failed for keyword: {}",
            keyword
        );
        assert_eq!(tokens[0].1, keyword, "Failed for keyword: {}", keyword);
    }
}

#[test]
fn test_contextual_keywords() {
    let keywords = vec![
        ("exports", SyntaxKind::EXPORTS_KW),
        ("module", SyntaxKind::MODULE_KW),
        ("open", SyntaxKind::OPEN_KW),
        ("opens", SyntaxKind::OPENS_KW),
        ("permits", SyntaxKind::PERMITS_KW),
        ("provides", SyntaxKind::PROVIDES_KW),
        ("record", SyntaxKind::RECORD_KW),
        ("requires", SyntaxKind::REQUIRES_KW),
        ("sealed", SyntaxKind::SEALED_KW),
        ("to", SyntaxKind::TO_KW),
        ("transitive", SyntaxKind::TRANSITIVE_KW),
        ("uses", SyntaxKind::USES_KW),
        ("var", SyntaxKind::VAR_KW),
        ("with", SyntaxKind::WITH_KW),
        ("yield", SyntaxKind::YIELD_KW),
    ];

    for (keyword, expected_kind) in keywords {
        let tokens = lex_without_trivia(keyword);
        assert_eq!(tokens.len(), 1, "Failed for keyword: {}", keyword);
        assert_eq!(
            tokens[0].0, expected_kind,
            "Failed for keyword: {}",
            keyword
        );
    }
}

#[test]
fn non_sealed_remains_three_expression_tokens() {
    let tokens = lex_without_trivia("non-sealed");
    assert_eq!(tokens.len(), 3);
    assert_eq!(tokens[0].0, SyntaxKind::IDENT);
    assert_eq!(tokens[0].1, "non");
    assert_eq!(tokens[1].0, SyntaxKind::MINUS);
    assert_eq!(tokens[2].0, SyntaxKind::SEALED_KW);
}

// ============================================================================
// Identifier Tests
// ============================================================================

#[test]
fn test_simple_identifiers() {
    let identifiers = vec!["x", "foo", "myVariable", "MyClass", "_private", "$dollar"];

    for ident in identifiers {
        let tokens = lex_without_trivia(ident);
        assert_eq!(tokens.len(), 1, "Failed for identifier: {}", ident);
        assert_eq!(
            tokens[0].0,
            SyntaxKind::IDENT,
            "Failed for identifier: {}",
            ident
        );
        assert_eq!(tokens[0].1, ident, "Failed for identifier: {}", ident);
    }
}

#[test]
fn test_identifiers_with_numbers() {
    let identifiers = vec!["var1", "test123", "item0"];

    for ident in identifiers {
        let tokens = lex_without_trivia(ident);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, SyntaxKind::IDENT);
    }
}

#[test]
fn test_identifier_vs_keyword() {
    // Identifiers that look similar to keywords but aren't
    let tokens = lex_without_trivia("classname");
    assert_eq!(tokens[0].0, SyntaxKind::IDENT);

    let tokens = lex_without_trivia("publicField");
    assert_eq!(tokens[0].0, SyntaxKind::IDENT);
}

// ============================================================================
// Integer Literal Tests
// ============================================================================

#[test]
fn test_decimal_integers() {
    let literals = vec!["0", "1", "42", "123456", "2147483647"];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL, "Failed for: {}", lit);
        assert_eq!(tokens[0].1, lit, "Failed for: {}", lit);
    }
}

#[test]
fn test_hexadecimal_integers() {
    let literals = vec!["0x0", "0xFF", "0xDEADBEEF", "0xCAFEBABE", "0x1A2B3C"];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL, "Failed for: {}", lit);
    }
}

#[test]
fn test_binary_integers() {
    let literals = vec!["0b0", "0b1", "0b1010", "0b11111111"];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL, "Failed for: {}", lit);
    }
}

#[test]
fn test_octal_integers() {
    let literals = vec!["00", "07", "0777", "0123"];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL, "Failed for: {}", lit);
    }
}

#[test]
fn test_integers_with_underscores() {
    let literals = vec!["1_000", "1_000_000", "0xFF_FF", "0b1010_1010"];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL, "Failed for: {}", lit);
    }
}

#[test]
fn test_long_literals() {
    let literals = vec!["0L", "42L", "123456789L", "0xFFL", "0b1010L"];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(tokens[0].0, SyntaxKind::LONG_LITERAL, "Failed for: {}", lit);
    }
}

#[test]
fn test_long_literals_lowercase() {
    let tokens = lex_without_trivia("42l");
    assert_eq!(tokens[0].0, SyntaxKind::LONG_LITERAL);
}

// ============================================================================
// Floating-Point Literal Tests
// ============================================================================

#[test]
fn test_double_literals() {
    let literals = vec!["0.0", "3.14", "1.0e10", "1.0E-5", ".5", "1."];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(
            tokens[0].0,
            SyntaxKind::DOUBLE_LITERAL,
            "Failed for: {}",
            lit
        );
    }
}

#[test]
fn test_float_literals() {
    let literals = vec!["0.0f", "3.14F", "1.0e10f", "2.5F"];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(
            tokens[0].0,
            SyntaxKind::FLOAT_LITERAL,
            "Failed for: {}",
            lit
        );
    }
}

#[test]
fn test_hex_float_literals() {
    let tokens = lex_without_trivia("0x1.8p2");
    assert_eq!(tokens[0].0, SyntaxKind::DOUBLE_LITERAL);
}

#[test]
fn test_floats_with_underscores() {
    let tokens = lex_without_trivia("3.14_15_92");
    assert_eq!(tokens[0].0, SyntaxKind::DOUBLE_LITERAL);
}

// ============================================================================
// Character Literal Tests
// ============================================================================

#[test]
fn test_simple_character_literals() {
    let literals = vec!["'a'", "'Z'", "'0'", "'@'"];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(tokens[0].0, SyntaxKind::CHAR_LITERAL, "Failed for: {}", lit);
    }
}

#[test]
fn test_escaped_character_literals() {
    let literals = vec![r"'\n'", r"'\t'", r"'\\'", r"'\''", "'\\\"'"];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(tokens[0].0, SyntaxKind::CHAR_LITERAL, "Failed for: {}", lit);
    }
}

#[test]
fn test_space_escape_literals() {
    for (source, kind) in [
        (r"'\s'", SyntaxKind::CHAR_LITERAL),
        (r#""before\safter""#, SyntaxKind::STRING_LITERAL),
    ] {
        assert_eq!(lex_without_trivia(source), vec![(kind, source.to_string())]);
    }
}

#[test]
fn test_character_literals_are_one_utf16_unit() {
    for source in ["'é'", "'漢'", r"'\uD83D'"] {
        assert_eq!(
            lex_without_trivia(source),
            vec![(SyntaxKind::CHAR_LITERAL, source.to_string())]
        );
    }
    assert!(lex_without_trivia("'😀'")
        .iter()
        .all(|(kind, _)| *kind != SyntaxKind::CHAR_LITERAL));
    let source = "\"😀\"";
    assert_eq!(
        lex_without_trivia(source),
        vec![(SyntaxKind::STRING_LITERAL, source.to_string())]
    );
}

#[test]
fn test_raw_line_terminators_are_not_quoted_literals() {
    for line_ending in ["\n", "\r", "\r\n"] {
        for source in [
            format!("'{}'", line_ending),
            format!("\"a{}b\"", line_ending),
        ] {
            let tokens = lex_without_trivia(&source);
            assert!(
                tokens.iter().any(|(kind, _)| *kind == SyntaxKind::ERROR),
                "raw line terminators must produce a lexical error: {source:?}: {tokens:?}"
            );
        }
    }
}

#[test]
fn test_octal_character_escape_limits() {
    for source in [r"'\0'", r"'\77'", r"'\377'"] {
        assert_eq!(
            lex_without_trivia(source),
            vec![(SyntaxKind::CHAR_LITERAL, source.to_string())]
        );
    }
    for source in [r"'\400'", r"'\777'"] {
        assert!(
            lex_without_trivia(source)
                .iter()
                .all(|(kind, _)| *kind != SyntaxKind::CHAR_LITERAL),
            "an octal character escape has at most three digits and must fit in a byte: {source}"
        );
    }
    // In a string, \\400 is the two-digit escape \\40 followed by an ordinary 0.
    let source = r#""\400""#;
    assert_eq!(
        lex_without_trivia(source),
        vec![(SyntaxKind::STRING_LITERAL, source.to_string())]
    );
}

#[test]
fn test_unicode_character_literals() {
    let tokens = lex_without_trivia(r"'\u0041'");
    assert_eq!(tokens[0].0, SyntaxKind::CHAR_LITERAL);
}

// ============================================================================
// String Literal Tests
// ============================================================================

#[test]
fn test_simple_string_literals() {
    let literals = vec![r#""""#, r#""hello""#, r#""Hello, World!""#];

    for lit in literals {
        let tokens = lex_without_trivia(lit);
        assert_eq!(tokens.len(), 1, "Failed for: {}", lit);
        assert_eq!(
            tokens[0].0,
            SyntaxKind::STRING_LITERAL,
            "Failed for: {}",
            lit
        );
    }
}

#[test]
fn test_string_with_escapes() {
    let source = r#""line1\nline2""#;
    let tokens = lex_without_trivia(source);
    assert_eq!(tokens[0].0, SyntaxKind::STRING_LITERAL);
}

#[test]
fn test_string_with_unicode() {
    let source = r#""Unicode: \u0041""#;
    let tokens = lex_without_trivia(source);
    assert_eq!(tokens[0].0, SyntaxKind::STRING_LITERAL);
}

// ============================================================================
// Boolean and Null Literal Tests
// ============================================================================

#[test]
fn test_boolean_literals() {
    let tokens = lex_without_trivia("true");
    assert_eq!(tokens[0].0, SyntaxKind::BOOLEAN_LITERAL);
    assert_eq!(tokens[0].1, "true");

    let tokens = lex_without_trivia("false");
    assert_eq!(tokens[0].0, SyntaxKind::BOOLEAN_LITERAL);
    assert_eq!(tokens[0].1, "false");
}

#[test]
fn test_null_literal() {
    let tokens = lex_without_trivia("null");
    assert_eq!(tokens[0].0, SyntaxKind::NULL_LITERAL);
    assert_eq!(tokens[0].1, "null");
}

// ============================================================================
// Operator Tests
// ============================================================================

#[test]
fn test_arithmetic_operators() {
    let operators = vec![
        ("+", SyntaxKind::PLUS),
        ("-", SyntaxKind::MINUS),
        ("*", SyntaxKind::STAR),
        ("/", SyntaxKind::SLASH),
        ("%", SyntaxKind::PERCENT),
    ];

    for (op, expected) in operators {
        let tokens = lex_without_trivia(op);
        assert_eq!(tokens[0].0, expected, "Failed for operator: {}", op);
    }
}

#[test]
fn test_comparison_operators() {
    let operators = vec![
        ("==", SyntaxKind::EQ_EQ),
        ("!=", SyntaxKind::BANG_EQ),
        ("<", SyntaxKind::LT),
        (">", SyntaxKind::GT),
        ("<=", SyntaxKind::LT_EQ),
        (">=", SyntaxKind::GT_EQ),
    ];

    for (op, expected) in operators {
        let tokens = lex_without_trivia(op);
        assert_eq!(tokens[0].0, expected, "Failed for operator: {}", op);
    }
}

#[test]
fn test_logical_operators() {
    let operators = vec![
        ("&&", SyntaxKind::AMP_AMP),
        ("||", SyntaxKind::PIPE_PIPE),
        ("!", SyntaxKind::BANG),
    ];

    for (op, expected) in operators {
        let tokens = lex_without_trivia(op);
        assert_eq!(tokens[0].0, expected, "Failed for operator: {}", op);
    }
}

#[test]
fn test_bitwise_operators() {
    let operators = vec![
        ("&", SyntaxKind::AMP),
        ("|", SyntaxKind::PIPE),
        ("^", SyntaxKind::CARET),
        ("~", SyntaxKind::TILDE),
        ("<<", SyntaxKind::SHL),
        (">>", SyntaxKind::SHR),
        (">>>", SyntaxKind::USHR),
    ];

    for (op, expected) in operators {
        let tokens = lex_without_trivia(op);
        assert_eq!(tokens[0].0, expected, "Failed for operator: {}", op);
    }
}

#[test]
fn test_assignment_operators() {
    let operators = vec![
        ("=", SyntaxKind::EQ),
        ("+=", SyntaxKind::PLUS_EQ),
        ("-=", SyntaxKind::MINUS_EQ),
        ("*=", SyntaxKind::STAR_EQ),
        ("/=", SyntaxKind::SLASH_EQ),
        ("%=", SyntaxKind::PERCENT_EQ),
        ("&=", SyntaxKind::AMP_EQ),
        ("|=", SyntaxKind::PIPE_EQ),
        ("^=", SyntaxKind::CARET_EQ),
        ("<<=", SyntaxKind::SHL_EQ),
        (">>=", SyntaxKind::SHR_EQ),
        (">>>=", SyntaxKind::USHR_EQ),
    ];

    for (op, expected) in operators {
        let tokens = lex_without_trivia(op);
        assert_eq!(tokens[0].0, expected, "Failed for operator: {}", op);
    }
}

#[test]
fn test_increment_decrement() {
    let tokens = lex_without_trivia("++");
    assert_eq!(tokens[0].0, SyntaxKind::PLUS_PLUS);

    let tokens = lex_without_trivia("--");
    assert_eq!(tokens[0].0, SyntaxKind::MINUS_MINUS);
}

#[test]
fn test_ternary_operator() {
    let tokens = lex_without_trivia("? :");
    assert_eq!(tokens[0].0, SyntaxKind::QUESTION);
    assert_eq!(tokens[1].0, SyntaxKind::COLON);
}

#[test]
fn test_lambda_and_method_reference() {
    let tokens = lex_without_trivia("->");
    assert_eq!(tokens[0].0, SyntaxKind::ARROW);

    let tokens = lex_without_trivia("::");
    assert_eq!(tokens[0].0, SyntaxKind::COLON_COLON);
}

// ============================================================================
// Punctuation Tests
// ============================================================================

#[test]
fn test_punctuation() {
    let punct = vec![
        ("(", SyntaxKind::L_PAREN),
        (")", SyntaxKind::R_PAREN),
        ("{", SyntaxKind::L_BRACE),
        ("}", SyntaxKind::R_BRACE),
        ("[", SyntaxKind::L_BRACKET),
        ("]", SyntaxKind::R_BRACKET),
        (";", SyntaxKind::SEMICOLON),
        (",", SyntaxKind::COMMA),
        (".", SyntaxKind::DOT),
        ("@", SyntaxKind::AT),
        ("...", SyntaxKind::ELLIPSIS),
    ];

    for (p, expected) in punct {
        let tokens = lex_without_trivia(p);
        assert_eq!(tokens[0].0, expected, "Failed for punctuation: {}", p);
    }
}

// ============================================================================
// Comment Tests
// ============================================================================

#[test]
fn test_line_comment() {
    let source = "// This is a comment\nclass Foo {}";
    let tokens = lex_all(source);

    assert_eq!(tokens[0].0, SyntaxKind::LINE_COMMENT);
    assert_eq!(tokens[0].1, "// This is a comment");
}

#[test]
fn test_block_comment() {
    let source = "/* block comment */ class Foo {}";
    let tokens = lex_all(source);

    assert_eq!(tokens[0].0, SyntaxKind::BLOCK_COMMENT);
    assert!(tokens[0].1.starts_with("/*"));
    assert!(tokens[0].1.ends_with("*/"));
}

#[test]
fn test_javadoc_comment() {
    let source = "/** javadoc comment */ class Foo {}";
    let tokens = lex_all(source);

    assert_eq!(tokens[0].0, SyntaxKind::DOC_COMMENT);
    assert!(tokens[0].1.starts_with("/**"));
}

#[test]
fn test_multiline_block_comment() {
    let source = r#"/*
     * Multi-line
     * comment
     */
    class Foo {}"#;
    let tokens = lex_all(source);

    assert_eq!(tokens[0].0, SyntaxKind::BLOCK_COMMENT);
}

#[test]
fn test_nested_comment_like_text() {
    // Block comments don't nest, so /* inside /* */ is just text
    let source = "/* comment /* not nested */ class Foo {}";
    let tokens = lex_all(source);

    // Should have a comment and then tokens
    assert_eq!(tokens[0].0, SyntaxKind::BLOCK_COMMENT);
}

// ============================================================================
// Whitespace Handling Tests
// ============================================================================

#[test]
fn test_whitespace_preservation() {
    let source = "int   x   =   42   ;";
    let tokens = lex_all(source);

    // Should preserve whitespace between tokens
    let mut ws_count = 0;
    for token in &tokens {
        if token.0 == SyntaxKind::WHITESPACE {
            ws_count += 1;
        }
    }

    assert!(ws_count > 0, "Whitespace should be preserved");
}

#[test]
fn test_mixed_whitespace() {
    let source = "int\t\tx\n=\r\n42;";
    let tokens = lex_all(source);

    // Should tokenize correctly despite mixed whitespace
    let non_trivia_count = tokens.iter().filter(|t| !t.0.is_trivia()).count();

    assert_eq!(non_trivia_count, 5); // int, x, =, 42, ;
}

// ============================================================================
// Text Range Tests
// ============================================================================

#[test]
fn test_text_ranges_are_contiguous() {
    let source = "int x = 42;";
    let tokens = lex_all(source);

    // Verify that ranges are contiguous (no gaps)
    for i in 1..tokens.len() {
        assert_eq!(
            tokens[i - 1].2.end(),
            tokens[i].2.start(),
            "Gap between tokens {} and {}",
            i - 1,
            i
        );
    }
}

#[test]
fn test_text_range_accuracy() {
    let source = "class";
    let tokens = lex_all(source);

    assert_eq!(u32::from(tokens[0].2.start()), 0u32);
    assert_eq!(u32::from(tokens[0].2.end()), 5u32);
}

// ============================================================================
// Edge Cases and Error Recovery
// ============================================================================

#[test]
fn test_invalid_characters() {
    let source = "int x = @#$;";
    let tokens = lex_all(source);

    // Should have error tokens for invalid characters
    let has_errors = tokens.iter().any(|t| t.0 == SyntaxKind::ERROR);
    assert!(
        has_errors,
        "Should contain error tokens for invalid characters"
    );
}

#[test]
fn test_incomplete_string() {
    // An unclosed string must produce an error without losing source text.
    let source = r#"String s = "unclosed"#;
    let tokens = lex_all(source);

    assert!(tokens.iter().any(|(kind, _, _)| *kind == SyntaxKind::ERROR));
    assert_eq!(
        tokens
            .iter()
            .map(|(_, text, _)| text.as_str())
            .collect::<String>(),
        source
    );
}

#[test]
fn test_operators_without_spaces() {
    let source = "a+b*c-d/e%f";
    let tokens = lex_without_trivia(source);

    assert_eq!(tokens.len(), 11); // a, +, b, *, c, -, d, /, e, %, f
}

#[test]
fn test_consecutive_operators() {
    let source = "+++";
    let tokens = lex_without_trivia(source);

    // Should parse as ++ and +
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].0, SyntaxKind::PLUS_PLUS);
    assert_eq!(tokens[1].0, SyntaxKind::PLUS);
}

#[test]
fn test_shift_vs_generic() {
    // >> could be shift operator or two closing angle brackets for generics
    let source = "List<List<String>>";
    let tokens = lex_without_trivia(source);

    // Lexer should tokenize >> as a single token
    // Parser needs to handle splitting it for generics
    let has_shr = tokens.iter().any(|t| t.0 == SyntaxKind::SHR);
    let has_gt = tokens.iter().any(|t| t.0 == SyntaxKind::GT);

    // Either interpretation is valid at lexer level
    assert!(has_shr || has_gt);
}

// ============================================================================
// Complex Real-World Examples
// ============================================================================

#[test]
fn test_hello_world_program() {
    let source = r#"
        public class HelloWorld {
            public static void main(String[] args) {
                System.out.println("Hello, World!");
            }
        }
    "#;

    let tokens = lex_without_trivia(source);

    // Should have many tokens
    assert!(tokens.len() > 20);

    // Verify some key tokens
    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::PUBLIC_KW));
    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::CLASS_KW));
    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::STATIC_KW));
    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::VOID_KW));
    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::STRING_LITERAL));
}

#[test]
fn test_generic_method_declaration() {
    let source = "public <T extends Comparable<T>> T max(T a, T b)";
    let tokens = lex_without_trivia(source);

    assert!(tokens.len() > 10);
    assert_eq!(tokens[0].0, SyntaxKind::PUBLIC_KW);
}

#[test]
fn test_lambda_expression() {
    let source = "(x, y) -> x + y";
    let tokens = lex_without_trivia(source);

    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::ARROW));
}

#[test]
fn test_annotation_usage() {
    let source = "@Override @Deprecated public void method() {}";
    let tokens = lex_without_trivia(source);

    let at_count = tokens.iter().filter(|(k, _)| *k == SyntaxKind::AT).count();
    assert_eq!(at_count, 2);
}

#[test]
fn test_var_keyword_java10() {
    let source = "var list = new ArrayList<String>();";
    let tokens = lex_without_trivia(source);

    assert_eq!(tokens[0].0, SyntaxKind::VAR_KW);
}

#[test]
fn test_record_declaration_java14() {
    let source = "public record Point(int x, int y) {}";
    let tokens = lex_without_trivia(source);

    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::RECORD_KW));
}

#[test]
fn test_sealed_class_java15() {
    let source = "public sealed class Shape permits Circle, Rectangle {}";
    let tokens = lex_without_trivia(source);

    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::SEALED_KW));
    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::PERMITS_KW));
}

#[test]
fn test_switch_expression_java12() {
    let source = "int result = switch(x) { case 1 -> 10; default -> 0; };";
    let tokens = lex_without_trivia(source);

    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::SWITCH_KW));
    assert!(tokens.iter().any(|(k, _)| *k == SyntaxKind::ARROW));
}

#[test]
fn test_block_comment_with_double_asterisks() {
    // Comment ending with **/ should still work
    let source = "/* content **/ class Foo {}";
    let tokens = lex_all(source);

    assert_eq!(
        tokens[0].0,
        SyntaxKind::BLOCK_COMMENT,
        "Should parse as block comment, got: {:?}",
        tokens[0].0
    );
    assert_eq!(tokens[0].1, "/* content **/");
}

#[test]
fn test_block_comment_with_asterisk_line() {
    // Common JavaDoc style with asterisk line prefix
    let source = r#"/*
 * Line 1
 * Line 2
 */"#;
    let tokens = lex_all(source);

    assert_eq!(tokens[0].0, SyntaxKind::BLOCK_COMMENT);
}

#[test]
fn test_block_comment_consecutive_asterisks() {
    // Multiple consecutive asterisks in comment body
    let source = "/* ***test*** */ class Foo {}";
    let tokens = lex_all(source);

    assert_eq!(
        tokens[0].0,
        SyntaxKind::BLOCK_COMMENT,
        "Should parse as block comment, got: {:?}",
        tokens[0].0
    );
}

#[test]
fn test_empty_block_comment() {
    let source = "/**/ class Foo {}";
    let tokens = lex_all(source);

    // Could be DOC_COMMENT or BLOCK_COMMENT depending on implementation
    assert!(
        tokens[0].0 == SyntaxKind::BLOCK_COMMENT || tokens[0].0 == SyntaxKind::DOC_COMMENT,
        "Should parse as comment, got: {:?}",
        tokens[0].0
    );
}

#[test]
fn test_javadoc_typical_format() {
    let source = r#"/**
 * This is a javadoc comment.
 * @param x the x value
 * @return the result
 */"#;
    let tokens = lex_all(source);

    assert_eq!(
        tokens[0].0,
        SyntaxKind::DOC_COMMENT,
        "Should parse as doc comment, got: {:?}",
        tokens[0].0
    );
}
