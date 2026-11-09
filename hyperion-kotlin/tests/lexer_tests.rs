// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Comprehensive Tests for Kotlin Lexer
//!
//! Tests tokenization of all Kotlin language features including keywords,
//! operators, literals, and edge cases.

use hyperion_kotlin::lexer::Lexer;
use hyperion_kotlin::syntax::SyntaxKind;

#[test]
fn test_empty_source() {
    assert_eq!(Lexer::new("").count(), 0);
}

#[test]
fn test_whitespace_only() {
    let lexer = Lexer::new("   \t\n  ");
    let tokens: Vec<_> = lexer.collect();

    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].0, SyntaxKind::WHITESPACE);
}

mod keyword_tests {
    use super::*;

    #[test]
    fn test_fun_keyword() {
        let tokens: Vec<_> = Lexer::new("fun").collect();
        assert_eq!(tokens[0].0, SyntaxKind::FUN_KW);
        assert_eq!(tokens[0].1, "fun");
    }

    #[test]
    fn test_val_var_keywords() {
        let tokens: Vec<_> = Lexer::new("val var").collect();
        assert_eq!(tokens[0].0, SyntaxKind::VAL_KW);
        assert_eq!(tokens[2].0, SyntaxKind::VAR_KW);
    }

    #[test]
    fn test_class_object_interface() {
        let tokens: Vec<_> = Lexer::new("class object interface")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::CLASS_KW);
        assert_eq!(tokens[1].0, SyntaxKind::OBJECT_KW);
        assert_eq!(tokens[2].0, SyntaxKind::INTERFACE_KW);
    }

    #[test]
    fn test_visibility_modifiers() {
        let tokens: Vec<_> = Lexer::new("public private protected internal")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::PUBLIC_KW);
        assert_eq!(tokens[1].0, SyntaxKind::PRIVATE_KW);
        assert_eq!(tokens[2].0, SyntaxKind::PROTECTED_KW);
        assert_eq!(tokens[3].0, SyntaxKind::INTERNAL_KW);
    }

    #[test]
    fn test_class_modifiers() {
        let tokens: Vec<_> = Lexer::new("open abstract data sealed")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::OPEN_KW);
        assert_eq!(tokens[1].0, SyntaxKind::ABSTRACT_KW);
        assert_eq!(tokens[2].0, SyntaxKind::DATA_KW);
        assert_eq!(tokens[3].0, SyntaxKind::SEALED_KW);
    }

    #[test]
    fn test_function_modifiers() {
        let tokens: Vec<_> = Lexer::new("suspend inline operator infix")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::SUSPEND_KW);
        assert_eq!(tokens[1].0, SyntaxKind::INLINE_KW);
        assert_eq!(tokens[2].0, SyntaxKind::OPERATOR_KW);
        assert_eq!(tokens[3].0, SyntaxKind::INFIX_KW);
    }

    #[test]
    fn test_control_flow_keywords() {
        let tokens: Vec<_> = Lexer::new("if else when for while return break continue")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::IF_KW);
        assert_eq!(tokens[1].0, SyntaxKind::ELSE_KW);
        assert_eq!(tokens[2].0, SyntaxKind::WHEN_KW);
        assert_eq!(tokens[3].0, SyntaxKind::FOR_KW);
        assert_eq!(tokens[4].0, SyntaxKind::WHILE_KW);
        assert_eq!(tokens[5].0, SyntaxKind::RETURN_KW);
        assert_eq!(tokens[6].0, SyntaxKind::BREAK_KW);
        assert_eq!(tokens[7].0, SyntaxKind::CONTINUE_KW);
    }

    #[test]
    fn test_exception_keywords() {
        let tokens: Vec<_> = Lexer::new("try catch finally throw")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::TRY_KW);
        assert_eq!(tokens[1].0, SyntaxKind::CATCH_KW);
        assert_eq!(tokens[2].0, SyntaxKind::FINALLY_KW);
        assert_eq!(tokens[3].0, SyntaxKind::THROW_KW);
    }

    #[test]
    fn test_type_keywords() {
        let tokens: Vec<_> = Lexer::new("as is in out")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::AS_KW);
        assert_eq!(tokens[1].0, SyntaxKind::IS_KW);
        assert_eq!(tokens[2].0, SyntaxKind::IN_KW);
        assert_eq!(tokens[3].0, SyntaxKind::OUT_KW);
    }

    #[test]
    fn test_special_keywords() {
        let tokens: Vec<_> = Lexer::new("companion init constructor override")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::COMPANION_KW);
        assert_eq!(tokens[1].0, SyntaxKind::INIT_KW);
        assert_eq!(tokens[2].0, SyntaxKind::CONSTRUCTOR_KW);
        assert_eq!(tokens[3].0, SyntaxKind::OVERRIDE_KW);
    }

    #[test]
    fn test_advanced_keywords() {
        let tokens: Vec<_> = Lexer::new("lateinit by typealias expect actual")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::LATEINIT_KW);
        assert_eq!(tokens[0].1, "lateinit");
        assert_eq!(tokens[1].0, SyntaxKind::BY_KW);
        assert_eq!(tokens[1].1, "by");
        assert_eq!(tokens[2].0, SyntaxKind::TYPEALIAS_KW);
        assert_eq!(tokens[2].1, "typealias");
        assert_eq!(tokens[3].0, SyntaxKind::EXPECT_KW);
        assert_eq!(tokens[3].1, "expect");
        assert_eq!(tokens[4].0, SyntaxKind::ACTUAL_KW);
        assert_eq!(tokens[4].1, "actual");
    }

    #[test]
    fn test_literal_keywords() {
        let tokens: Vec<_> = Lexer::new("true false null this super")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::TRUE_KW);
        assert_eq!(tokens[1].0, SyntaxKind::FALSE_KW);
        assert_eq!(tokens[2].0, SyntaxKind::NULL_KW);
        assert_eq!(tokens[3].0, SyntaxKind::THIS_KW);
        assert_eq!(tokens[4].0, SyntaxKind::SUPER_KW);
    }

    #[test]
    fn test_package_import_keywords() {
        let tokens: Vec<_> = Lexer::new("package import")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::PACKAGE_KW);
        assert_eq!(tokens[1].0, SyntaxKind::IMPORT_KW);
    }
}

mod operator_tests {
    use super::*;

    #[test]
    fn test_arithmetic_operators() {
        let tokens: Vec<_> = Lexer::new("+ - * / %")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::PLUS);
        assert_eq!(tokens[1].0, SyntaxKind::MINUS);
        assert_eq!(tokens[2].0, SyntaxKind::STAR);
        assert_eq!(tokens[3].0, SyntaxKind::SLASH);
        assert_eq!(tokens[4].0, SyntaxKind::PERCENT);
    }

    #[test]
    fn test_comparison_operators() {
        let tokens: Vec<_> = Lexer::new("== != < > <= >=")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::EQEQ);
        assert_eq!(tokens[1].0, SyntaxKind::BANGEQ);
        assert_eq!(tokens[2].0, SyntaxKind::LT);
        assert_eq!(tokens[3].0, SyntaxKind::GT);
        assert_eq!(tokens[4].0, SyntaxKind::LTEQ);
        assert_eq!(tokens[5].0, SyntaxKind::GTEQ);
    }

    #[test]
    fn test_logical_operators() {
        let tokens: Vec<_> = Lexer::new("&& || !")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::AMPAMP);
        assert_eq!(tokens[1].0, SyntaxKind::PIPEPIPE);
        assert_eq!(tokens[2].0, SyntaxKind::BANG);
    }

    #[test]
    fn test_kotlin_specific_operators() {
        let tokens: Vec<_> = Lexer::new("-> ?: ?. !!")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::ARROW);
        assert_eq!(tokens[1].0, SyntaxKind::ELVIS);
        assert_eq!(tokens[2].0, SyntaxKind::SAFE_CALL);
        assert_eq!(tokens[3].0, SyntaxKind::NOT_NULL);
    }

    #[test]
    fn test_range_operators() {
        let tokens: Vec<_> = Lexer::new(".. ..<")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::RANGE);
        assert_eq!(tokens[1].0, SyntaxKind::RANGE_UNTIL);
    }

    #[test]
    fn test_increment_decrement() {
        let tokens: Vec<_> = Lexer::new("++ --")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::PLUSPLUS);
        assert_eq!(tokens[1].0, SyntaxKind::MINUSMINUS);
    }

    #[test]
    fn test_member_reference() {
        let tokens: Vec<_> = Lexer::new("::").collect();

        assert_eq!(tokens[0].0, SyntaxKind::COLONCOLON);
        assert_eq!(tokens[0].1, "::");
    }

    #[test]
    fn test_assignment() {
        let tokens: Vec<_> = Lexer::new("=").collect();
        assert_eq!(tokens[0].0, SyntaxKind::EQ);
    }

    #[test]
    fn test_double_arrow() {
        let tokens: Vec<_> = Lexer::new("=>").collect();
        assert_eq!(tokens[0].0, SyntaxKind::DOUBLE_ARROW);
    }

    #[test]
    fn test_not_in_not_is_operators() {
        let tokens: Vec<_> = Lexer::new("!in !is")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::NOT_IN);
        assert_eq!(tokens[0].1, "!in");
        assert_eq!(tokens[1].0, SyntaxKind::NOT_IS);
        assert_eq!(tokens[1].1, "!is");
    }

    #[test]
    fn test_safe_cast_operator() {
        let tokens: Vec<_> = Lexer::new("as?")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::SAFE_CAST);
        assert_eq!(tokens[0].1, "as?");
    }

    #[test]
    fn test_type_check_operators_in_context() {
        // Test that !in, !is, as? work correctly in realistic code
        let source = "if (x !is String) return; if (y !in list) break; val z = obj as? Type";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        // Find the !is operator
        let not_is_pos = tokens
            .iter()
            .position(|(k, _, _)| *k == SyntaxKind::NOT_IS)
            .unwrap();
        assert_eq!(tokens[not_is_pos].1, "!is");

        // Find the !in operator
        let not_in_pos = tokens
            .iter()
            .position(|(k, _, _)| *k == SyntaxKind::NOT_IN)
            .unwrap();
        assert_eq!(tokens[not_in_pos].1, "!in");

        // Find the as? operator
        let safe_cast_pos = tokens
            .iter()
            .position(|(k, _, _)| *k == SyntaxKind::SAFE_CAST)
            .unwrap();
        assert_eq!(tokens[safe_cast_pos].1, "as?");
    }
}

mod literal_tests {
    use super::*;

    #[test]
    fn test_integer_literals() {
        let tokens: Vec<_> = Lexer::new("42 0 123456789")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[0].1, "42");
        assert_eq!(tokens[1].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::INT_LITERAL);
    }

    #[test]
    fn test_hex_literals() {
        let tokens: Vec<_> = Lexer::new("0xFF 0x00 0xDEADBEEF")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[0].1, "0xFF");
        assert_eq!(tokens[1].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::INT_LITERAL);
    }

    #[test]
    fn test_binary_literals() {
        let tokens: Vec<_> = Lexer::new("0b101010 0b11111111")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[0].1, "0b101010");
        assert_eq!(tokens[1].0, SyntaxKind::INT_LITERAL);
    }

    #[test]
    fn test_long_literals() {
        let tokens: Vec<_> = Lexer::new("42L 0xFFL 0b1010L")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::LONG_LITERAL);
        assert_eq!(tokens[0].1, "42L");
        assert_eq!(tokens[1].0, SyntaxKind::LONG_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::LONG_LITERAL);
    }

    #[test]
    fn test_float_literals() {
        let tokens: Vec<_> = Lexer::new("3.14f 0.5f 1f")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::FLOAT_LITERAL);
        assert_eq!(tokens[0].1, "3.14f");
        assert_eq!(tokens[1].0, SyntaxKind::FLOAT_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::FLOAT_LITERAL);
    }

    #[test]
    fn test_double_literals() {
        let tokens: Vec<_> = Lexer::new("3.14 0.5 1.0")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::DOUBLE_LITERAL);
        assert_eq!(tokens[1].0, SyntaxKind::DOUBLE_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::DOUBLE_LITERAL);
    }

    #[test]
    fn test_scientific_notation() {
        let tokens: Vec<_> = Lexer::new("1e10 1.5e-3 2E5f")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::DOUBLE_LITERAL);
        assert_eq!(tokens[1].0, SyntaxKind::DOUBLE_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::FLOAT_LITERAL);
    }

    #[test]
    fn test_underscores_in_numbers() {
        let tokens: Vec<_> = Lexer::new("1_000_000 0xFF_FF_FF 0b1010_1010")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[1].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::INT_LITERAL);
    }

    #[test]
    fn test_string_literals() {
        let tokens: Vec<_> = Lexer::new(r#""hello" "world" "with spaces""#)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::STRING_LITERAL);
        assert_eq!(tokens[0].1, r#""hello""#);
        assert_eq!(tokens[1].0, SyntaxKind::STRING_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::STRING_LITERAL);
    }

    #[test]
    fn test_string_with_escapes() {
        let tokens: Vec<_> = Lexer::new(r#""hello\nworld" "tab\there""#)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::STRING_LITERAL);
        assert_eq!(tokens[1].0, SyntaxKind::STRING_LITERAL);
    }

    #[test]
    fn test_raw_string_literals() {
        let tokens: Vec<_> = Lexer::new(
            r#""""raw string""" """multi
line"""#,
        )
        .collect();

        assert_eq!(tokens[0].0, SyntaxKind::RAW_STRING_LITERAL);
    }

    #[test]
    fn test_character_literals() {
        let tokens: Vec<_> = Lexer::new(r"'a' 'Z' '9' '\n' '\t'")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::CHAR_LITERAL);
        assert_eq!(tokens[0].1, "'a'");
        assert_eq!(tokens[1].0, SyntaxKind::CHAR_LITERAL);
        assert_eq!(tokens[2].0, SyntaxKind::CHAR_LITERAL);
        assert_eq!(tokens[3].0, SyntaxKind::CHAR_LITERAL);
        assert_eq!(tokens[4].0, SyntaxKind::CHAR_LITERAL);
    }
}

mod identifier_tests {
    use super::*;

    #[test]
    fn test_simple_identifiers() {
        let tokens: Vec<_> = Lexer::new("foo bar baz_123 _underscore")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::IDENT);
        assert_eq!(tokens[0].1, "foo");
        assert_eq!(tokens[1].0, SyntaxKind::IDENT);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
        assert_eq!(tokens[3].0, SyntaxKind::IDENT);
    }

    #[test]
    fn test_backtick_identifiers() {
        let tokens: Vec<_> = Lexer::new("`fun` `reserved word` `with spaces`")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::BACKTICK_IDENT);
        assert_eq!(tokens[0].1, "`fun`");
        assert_eq!(tokens[1].0, SyntaxKind::BACKTICK_IDENT);
        assert_eq!(tokens[2].0, SyntaxKind::BACKTICK_IDENT);
    }

    #[test]
    fn test_identifier_vs_keyword() {
        let tokens: Vec<_> = Lexer::new("fun function funky")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::FUN_KW);
        assert_eq!(tokens[1].0, SyntaxKind::IDENT);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
    }

    #[test]
    fn test_unicode_identifiers() {
        let tokens: Vec<_> = Lexer::new("世界 café αβγ")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        // Kotlin identifiers may contain any Unicode letter.
        let kinds_and_text: Vec<_> = tokens.iter().map(|(k, t, _)| (*k, *t)).collect();
        assert_eq!(
            kinds_and_text,
            vec![
                (SyntaxKind::IDENT, "世界"),
                (SyntaxKind::IDENT, "café"),
                (SyntaxKind::IDENT, "αβγ"),
            ]
        );
    }
}

mod comment_tests {
    use super::*;

    #[test]
    fn test_line_comment() {
        let tokens: Vec<_> = Lexer::new("// this is a comment\ncode").collect();

        assert_eq!(tokens[0].0, SyntaxKind::LINE_COMMENT);
        assert_eq!(tokens[0].1, "// this is a comment");
        assert_eq!(tokens[1].0, SyntaxKind::WHITESPACE);
    }

    #[test]
    fn test_block_comment() {
        let tokens: Vec<_> = Lexer::new("/* block comment */ code").collect();

        assert_eq!(tokens[0].0, SyntaxKind::BLOCK_COMMENT);
        assert_eq!(tokens[0].1, "/* block comment */");
    }

    #[test]
    fn test_multiline_block_comment() {
        let source = "/* line 1\nline 2\nline 3 */";
        let tokens: Vec<_> = Lexer::new(source).collect();

        assert_eq!(tokens[0].0, SyntaxKind::BLOCK_COMMENT);
    }

    #[test]
    fn test_nested_block_comments_not_supported() {
        // Kotlin doesn't support nested block comments
        let source = "/* outer /* inner */ still in comment? */";
        let tokens: Vec<_> = Lexer::new(source).collect();

        // Should close at first */
        assert_eq!(tokens[0].0, SyntaxKind::BLOCK_COMMENT);
    }

    #[test]
    fn test_comment_at_end_of_file() {
        // Should have code, whitespace, and comment
        assert!(Lexer::new("code // comment").count() >= 3);
    }
}

mod delimiter_tests {
    use super::*;

    #[test]
    fn test_parentheses() {
        let tokens: Vec<_> = Lexer::new("( )")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::L_PAREN);
        assert_eq!(tokens[1].0, SyntaxKind::R_PAREN);
    }

    #[test]
    fn test_braces() {
        let tokens: Vec<_> = Lexer::new("{ }")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::L_BRACE);
        assert_eq!(tokens[1].0, SyntaxKind::R_BRACE);
    }

    #[test]
    fn test_brackets() {
        let tokens: Vec<_> = Lexer::new("[ ]")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::L_BRACKET);
        assert_eq!(tokens[1].0, SyntaxKind::R_BRACKET);
    }

    #[test]
    fn test_punctuation() {
        let tokens: Vec<_> = Lexer::new("; : , . ? @")
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::SEMICOLON);
        assert_eq!(tokens[1].0, SyntaxKind::COLON);
        assert_eq!(tokens[2].0, SyntaxKind::COMMA);
        assert_eq!(tokens[3].0, SyntaxKind::DOT);
        assert_eq!(tokens[4].0, SyntaxKind::QUESTION);
        assert_eq!(tokens[5].0, SyntaxKind::AT);
    }
}

mod integration_tests {
    use super::*;

    #[test]
    fn test_simple_function() {
        let source = "fun main() { }";
        let tokens: Vec<_> = Lexer::new(source).collect();

        assert_eq!(tokens[0].0, SyntaxKind::FUN_KW);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
        // More tokens follow...
    }

    #[test]
    fn test_property_declaration() {
        let source = "val x: Int = 42";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::VAL_KW);
        assert_eq!(tokens[1].0, SyntaxKind::IDENT);
        assert_eq!(tokens[2].0, SyntaxKind::COLON);
        assert_eq!(tokens[3].0, SyntaxKind::IDENT);
        assert_eq!(tokens[4].0, SyntaxKind::EQ);
        assert_eq!(tokens[5].0, SyntaxKind::INT_LITERAL);
    }

    #[test]
    fn test_lambda_expression() {
        let source = "{ x -> x + 1 }";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::L_BRACE);
        assert_eq!(tokens[1].0, SyntaxKind::IDENT);
        assert_eq!(tokens[2].0, SyntaxKind::ARROW);
    }

    #[test]
    fn test_class_declaration() {
        let source = "data class Person(val name: String)";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::DATA_KW);
        assert_eq!(tokens[1].0, SyntaxKind::CLASS_KW);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
    }

    #[test]
    fn test_nullable_type() {
        let source = "String?";
        let tokens: Vec<_> = Lexer::new(source).collect();

        assert_eq!(tokens[0].0, SyntaxKind::IDENT);
        assert_eq!(tokens[1].0, SyntaxKind::QUESTION);
    }

    #[test]
    fn test_safe_call_chain() {
        let source = "obj?.prop?.method()";
        let tokens: Vec<_> = Lexer::new(source).collect();

        assert_eq!(tokens[0].0, SyntaxKind::IDENT);
        assert_eq!(tokens[1].0, SyntaxKind::SAFE_CALL);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
        assert_eq!(tokens[3].0, SyntaxKind::SAFE_CALL);
    }

    #[test]
    fn test_range_expression() {
        let source = "1..10 0..<100";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[1].0, SyntaxKind::RANGE);
        assert_eq!(tokens[2].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[3].0, SyntaxKind::INT_LITERAL);
        assert_eq!(tokens[4].0, SyntaxKind::RANGE_UNTIL);
        assert_eq!(tokens[5].0, SyntaxKind::INT_LITERAL);
    }

    #[test]
    fn test_elvis_operator() {
        let source = "value ?: default";
        let tokens: Vec<_> = Lexer::new(source)
            .filter(|(kind, _, _)| !kind.is_trivia())
            .collect();

        assert_eq!(tokens[0].0, SyntaxKind::IDENT);
        assert_eq!(tokens[1].0, SyntaxKind::ELVIS);
        assert_eq!(tokens[2].0, SyntaxKind::IDENT);
    }

    #[test]
    fn test_complete_file() {
        let source = r#"
package com.example

import kotlin.collections.List

data class Person(val name: String, val age: Int)

fun main() {
    val person = Person("Alice", 30)
    println(person.name)
}
        "#;

        let tokens: Vec<_> = Lexer::new(source).collect();

        // Should tokenize without errors
        assert!(!tokens.is_empty());

        // First non-trivia token should be package keyword
        let first_keyword = tokens
            .iter()
            .find(|(kind, _, _)| !kind.is_trivia())
            .unwrap();
        assert_eq!(first_keyword.0, SyntaxKind::PACKAGE_KW);
    }
}

mod error_recovery_tests {
    use super::*;

    #[test]
    fn test_invalid_character() {
        let source = "fun `";
        // Should produce tokens, potentially including ERROR
        assert!(Lexer::new(source).next().is_some());
    }

    #[test]
    fn test_unclosed_string() {
        let source = r#""unclosed"#;
        // Lexer behavior on unclosed string
        assert!(Lexer::new(source).next().is_some());
    }

    #[test]
    fn test_unclosed_block_comment() {
        let source = "/* unclosed";
        // Lexer behavior on unclosed comment
        assert!(Lexer::new(source).next().is_some());
    }
}

mod text_range_tests {
    use super::*;

    #[test]
    fn test_token_ranges() {
        let source = "fun main() { }";
        let tokens: Vec<_> = Lexer::new(source).collect();

        // Verify ranges are continuous and valid
        for (i, (_, _, range)) in tokens.iter().enumerate() {
            assert!(range.start() <= range.end());
            if i > 0 {
                // Ranges should be continuous (lossless)
                let prev_end = tokens[i - 1].2.end();
                assert_eq!(prev_end, range.start());
            }
        }
    }

    #[test]
    fn test_token_text_matches_range() {
        let source = "fun main() { }";
        let tokens: Vec<_> = Lexer::new(source).collect();

        for (_, text, range) in tokens {
            let start: usize = range.start().into();
            let end: usize = range.end().into();
            assert_eq!(&source[start..end], text);
        }
    }
}

// String interpolation is lexed as a single STRING_LITERAL token.
// The navigation provider handles extracting identifiers from within the string.
#[test]
fn test_string_interpolation_tokenization() {
    let source = r#""Hello, $name!""#;
    let tokens: Vec<_> = Lexer::new(source).collect();

    // String interpolation is lexed as a single STRING_LITERAL
    assert_eq!(tokens[0].0, SyntaxKind::STRING_LITERAL);
    assert_eq!(tokens[0].1, r#""Hello, $name!""#);
}
