// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for token classification utilities
//!
//! This test suite covers:
//! - Token type classification (keywords, operators, literals, etc.)
//! - Modifier identification
//! - Statement keyword detection
//! - Expression keyword detection
//! - Primitive type identification
//! - `SyntaxKind` helper methods

use hyperion_parser::syntax::SyntaxKind;
use hyperion_parser::token::TokenClassification;

// ============================================================================
// Basic Token Classification Tests
// ============================================================================

#[test]
fn test_trivia_classification() {
    assert!(SyntaxKind::WHITESPACE.is_trivia());
    assert!(SyntaxKind::LINE_COMMENT.is_trivia());
    assert!(SyntaxKind::BLOCK_COMMENT.is_trivia());
    assert!(SyntaxKind::DOC_COMMENT.is_trivia());

    // Non-trivia tokens
    assert!(!SyntaxKind::CLASS_KW.is_trivia());
    assert!(!SyntaxKind::IDENT.is_trivia());
    assert!(!SyntaxKind::INT_LITERAL.is_trivia());
}

#[test]
fn test_keyword_classification() {
    // Standard keywords
    assert!(SyntaxKind::CLASS_KW.is_keyword());
    assert!(SyntaxKind::PUBLIC_KW.is_keyword());
    assert!(SyntaxKind::STATIC_KW.is_keyword());
    assert!(SyntaxKind::VOID_KW.is_keyword());
    assert!(SyntaxKind::IF_KW.is_keyword());
    assert!(SyntaxKind::WHILE_KW.is_keyword());

    // Contextual keywords (Java 9+)
    assert!(SyntaxKind::VAR_KW.is_keyword());
    assert!(SyntaxKind::RECORD_KW.is_keyword());
    assert!(SyntaxKind::SEALED_KW.is_keyword());
    assert!(SyntaxKind::PERMITS_KW.is_keyword());

    // Non-keywords
    assert!(!SyntaxKind::IDENT.is_keyword());
    assert!(!SyntaxKind::INT_LITERAL.is_keyword());
    assert!(!SyntaxKind::PLUS.is_keyword());
}

#[test]
fn test_literal_classification() {
    assert!(SyntaxKind::INT_LITERAL.is_literal());
    assert!(SyntaxKind::LONG_LITERAL.is_literal());
    assert!(SyntaxKind::FLOAT_LITERAL.is_literal());
    assert!(SyntaxKind::DOUBLE_LITERAL.is_literal());
    assert!(SyntaxKind::CHAR_LITERAL.is_literal());
    assert!(SyntaxKind::STRING_LITERAL.is_literal());
    assert!(SyntaxKind::TEXT_BLOCK_LITERAL.is_literal());
    assert!(SyntaxKind::BOOLEAN_LITERAL.is_literal());
    assert!(SyntaxKind::NULL_LITERAL.is_literal());

    // Non-literals
    assert!(!SyntaxKind::IDENT.is_literal());
    assert!(!SyntaxKind::CLASS_KW.is_literal());
    assert!(!SyntaxKind::PLUS.is_literal());
}

#[test]
fn test_operator_classification() {
    // Arithmetic operators
    assert!(SyntaxKind::PLUS.is_operator());
    assert!(SyntaxKind::MINUS.is_operator());
    assert!(SyntaxKind::STAR.is_operator());
    assert!(SyntaxKind::SLASH.is_operator());
    assert!(SyntaxKind::PERCENT.is_operator());

    // Comparison operators
    assert!(SyntaxKind::EQ_EQ.is_operator());
    assert!(SyntaxKind::BANG_EQ.is_operator());
    assert!(SyntaxKind::LT.is_operator());
    assert!(SyntaxKind::GT.is_operator());
    assert!(SyntaxKind::LT_EQ.is_operator());
    assert!(SyntaxKind::GT_EQ.is_operator());

    // Logical operators
    assert!(SyntaxKind::AMP_AMP.is_operator());
    assert!(SyntaxKind::PIPE_PIPE.is_operator());
    assert!(SyntaxKind::BANG.is_operator());

    // Bitwise operators
    assert!(SyntaxKind::AMP.is_operator());
    assert!(SyntaxKind::PIPE.is_operator());
    assert!(SyntaxKind::CARET.is_operator());
    assert!(SyntaxKind::TILDE.is_operator());
    assert!(SyntaxKind::SHL.is_operator());
    assert!(SyntaxKind::SHR.is_operator());
    assert!(SyntaxKind::USHR.is_operator());

    // Assignment operators
    assert!(SyntaxKind::EQ.is_operator());
    assert!(SyntaxKind::PLUS_EQ.is_operator());
    assert!(SyntaxKind::MINUS_EQ.is_operator());

    // Other operators
    assert!(SyntaxKind::QUESTION.is_operator());
    assert!(SyntaxKind::COLON.is_operator());
    assert!(SyntaxKind::ARROW.is_operator());
    assert!(SyntaxKind::COLON_COLON.is_operator());
    assert!(SyntaxKind::DOT.is_operator());
    assert!(SyntaxKind::ELLIPSIS.is_operator());

    // Non-operators
    assert!(!SyntaxKind::CLASS_KW.is_operator());
    assert!(!SyntaxKind::IDENT.is_operator());
    assert!(!SyntaxKind::L_PAREN.is_operator());
}

#[test]
fn test_punctuation_classification() {
    assert!(SyntaxKind::L_PAREN.is_punctuation());
    assert!(SyntaxKind::R_PAREN.is_punctuation());
    assert!(SyntaxKind::L_BRACE.is_punctuation());
    assert!(SyntaxKind::R_BRACE.is_punctuation());
    assert!(SyntaxKind::L_BRACKET.is_punctuation());
    assert!(SyntaxKind::R_BRACKET.is_punctuation());
    assert!(SyntaxKind::SEMICOLON.is_punctuation());
    assert!(SyntaxKind::COMMA.is_punctuation());

    // Non-punctuation
    assert!(!SyntaxKind::CLASS_KW.is_punctuation());
    assert!(!SyntaxKind::PLUS.is_punctuation());
    assert!(!SyntaxKind::IDENT.is_punctuation());
}

#[test]
fn test_node_classification() {
    // Composite nodes
    assert!(SyntaxKind::SOURCE_FILE.is_node());
    assert!(SyntaxKind::CLASS_DECL.is_node());
    assert!(SyntaxKind::METHOD_DECL.is_node());
    assert!(SyntaxKind::FIELD_DECL.is_node());
    assert!(SyntaxKind::IF_STMT.is_node());
    assert!(SyntaxKind::WHILE_STMT.is_node());
    assert!(SyntaxKind::BINARY_EXPR.is_node());

    // Tokens (not nodes)
    assert!(!SyntaxKind::CLASS_KW.is_node());
    assert!(!SyntaxKind::IDENT.is_node());
    assert!(!SyntaxKind::INT_LITERAL.is_node());
    assert!(!SyntaxKind::PLUS.is_node());
}

// ============================================================================
// Modifier Classification Tests
// ============================================================================

#[test]
fn test_visibility_modifiers() {
    assert!(SyntaxKind::PUBLIC_KW.is_modifier());
    assert!(SyntaxKind::PRIVATE_KW.is_modifier());
    assert!(SyntaxKind::PROTECTED_KW.is_modifier());
}

#[test]
fn test_class_modifiers() {
    assert!(SyntaxKind::ABSTRACT_KW.is_modifier());
    assert!(SyntaxKind::FINAL_KW.is_modifier());
    assert!(SyntaxKind::STATIC_KW.is_modifier());
    assert!(SyntaxKind::SEALED_KW.is_modifier());
    assert!(SyntaxKind::NON_SEALED_KW.is_modifier());
}

#[test]
fn test_field_modifiers() {
    assert!(SyntaxKind::TRANSIENT_KW.is_modifier());
    assert!(SyntaxKind::VOLATILE_KW.is_modifier());
}

#[test]
fn test_method_modifiers() {
    assert!(SyntaxKind::SYNCHRONIZED_KW.is_modifier());
    assert!(SyntaxKind::NATIVE_KW.is_modifier());
    assert!(SyntaxKind::STRICTFP_KW.is_modifier());
    assert!(SyntaxKind::DEFAULT_KW.is_modifier());
}

#[test]
fn test_all_modifiers() {
    let modifiers = vec![
        SyntaxKind::PUBLIC_KW,
        SyntaxKind::PRIVATE_KW,
        SyntaxKind::PROTECTED_KW,
        SyntaxKind::STATIC_KW,
        SyntaxKind::FINAL_KW,
        SyntaxKind::ABSTRACT_KW,
        SyntaxKind::NATIVE_KW,
        SyntaxKind::SYNCHRONIZED_KW,
        SyntaxKind::TRANSIENT_KW,
        SyntaxKind::VOLATILE_KW,
        SyntaxKind::STRICTFP_KW,
        SyntaxKind::DEFAULT_KW,
        SyntaxKind::SEALED_KW,
        SyntaxKind::NON_SEALED_KW,
    ];

    for modifier in modifiers {
        assert!(modifier.is_modifier(), "{modifier:?} should be a modifier");
    }
}

#[test]
fn test_non_modifiers() {
    assert!(!SyntaxKind::CLASS_KW.is_modifier());
    assert!(!SyntaxKind::INTERFACE_KW.is_modifier());
    assert!(!SyntaxKind::ENUM_KW.is_modifier());
    assert!(!SyntaxKind::IF_KW.is_modifier());
    assert!(!SyntaxKind::WHILE_KW.is_modifier());
}

// ============================================================================
// Primitive Type Tests
// ============================================================================

#[test]
fn test_numeric_primitive_types() {
    assert!(SyntaxKind::BYTE_KW.is_primitive_type());
    assert!(SyntaxKind::SHORT_KW.is_primitive_type());
    assert!(SyntaxKind::INT_KW.is_primitive_type());
    assert!(SyntaxKind::LONG_KW.is_primitive_type());
    assert!(SyntaxKind::FLOAT_KW.is_primitive_type());
    assert!(SyntaxKind::DOUBLE_KW.is_primitive_type());
}

#[test]
fn test_boolean_and_char_primitive_types() {
    assert!(SyntaxKind::BOOLEAN_KW.is_primitive_type());
    assert!(SyntaxKind::CHAR_KW.is_primitive_type());
}

#[test]
fn test_void_as_primitive_type() {
    assert!(SyntaxKind::VOID_KW.is_primitive_type());
}

#[test]
fn test_all_primitive_types() {
    let primitives = vec![
        SyntaxKind::BOOLEAN_KW,
        SyntaxKind::BYTE_KW,
        SyntaxKind::SHORT_KW,
        SyntaxKind::INT_KW,
        SyntaxKind::LONG_KW,
        SyntaxKind::CHAR_KW,
        SyntaxKind::FLOAT_KW,
        SyntaxKind::DOUBLE_KW,
        SyntaxKind::VOID_KW,
    ];

    for prim in primitives {
        assert!(
            prim.is_primitive_type(),
            "{prim:?} should be a primitive type"
        );
    }
}

#[test]
fn test_non_primitive_types() {
    assert!(!SyntaxKind::CLASS_KW.is_primitive_type());
    assert!(!SyntaxKind::INTERFACE_KW.is_primitive_type());
    assert!(!SyntaxKind::IDENT.is_primitive_type());
    assert!(!SyntaxKind::VAR_KW.is_primitive_type());
}

// ============================================================================
// Statement Keyword Tests
// ============================================================================

#[test]
fn test_conditional_statement_keywords() {
    assert!(SyntaxKind::IF_KW.is_statement_keyword());
    assert!(SyntaxKind::ELSE_KW.is_statement_keyword());
    assert!(SyntaxKind::SWITCH_KW.is_statement_keyword());
    assert!(SyntaxKind::CASE_KW.is_statement_keyword());
}

#[test]
fn test_loop_statement_keywords() {
    assert!(SyntaxKind::WHILE_KW.is_statement_keyword());
    assert!(SyntaxKind::DO_KW.is_statement_keyword());
    assert!(SyntaxKind::FOR_KW.is_statement_keyword());
}

#[test]
fn test_control_flow_statement_keywords() {
    assert!(SyntaxKind::BREAK_KW.is_statement_keyword());
    assert!(SyntaxKind::CONTINUE_KW.is_statement_keyword());
    assert!(SyntaxKind::RETURN_KW.is_statement_keyword());
}

#[test]
fn test_exception_statement_keywords() {
    assert!(SyntaxKind::THROW_KW.is_statement_keyword());
    assert!(SyntaxKind::TRY_KW.is_statement_keyword());
    assert!(SyntaxKind::CATCH_KW.is_statement_keyword());
    assert!(SyntaxKind::FINALLY_KW.is_statement_keyword());
}

#[test]
fn test_other_statement_keywords() {
    assert!(SyntaxKind::ASSERT_KW.is_statement_keyword());
    assert!(SyntaxKind::SYNCHRONIZED_KW.is_statement_keyword());
    assert!(SyntaxKind::YIELD_KW.is_statement_keyword());
}

#[test]
fn test_default_as_statement_keyword() {
    // DEFAULT can be both a modifier and a statement keyword (in switch)
    assert!(SyntaxKind::DEFAULT_KW.is_statement_keyword());
}

#[test]
fn test_non_statement_keywords() {
    assert!(!SyntaxKind::CLASS_KW.is_statement_keyword());
    assert!(!SyntaxKind::INTERFACE_KW.is_statement_keyword());
    assert!(!SyntaxKind::PUBLIC_KW.is_statement_keyword());
    assert!(!SyntaxKind::STATIC_KW.is_statement_keyword());
}

// ============================================================================
// Expression Keyword Tests
// ============================================================================

#[test]
fn test_object_creation_expression_keyword() {
    assert!(SyntaxKind::NEW_KW.is_expression_keyword());
}

#[test]
fn test_reference_expression_keywords() {
    assert!(SyntaxKind::THIS_KW.is_expression_keyword());
    assert!(SyntaxKind::SUPER_KW.is_expression_keyword());
}

#[test]
fn test_type_check_expression_keyword() {
    assert!(SyntaxKind::INSTANCEOF_KW.is_expression_keyword());
}

#[test]
fn test_all_expression_keywords() {
    let expr_keywords = vec![
        SyntaxKind::NEW_KW,
        SyntaxKind::THIS_KW,
        SyntaxKind::SUPER_KW,
        SyntaxKind::INSTANCEOF_KW,
    ];

    for kw in expr_keywords {
        assert!(
            kw.is_expression_keyword(),
            "{kw:?} should be an expression keyword"
        );
    }
}

#[test]
fn test_non_expression_keywords() {
    assert!(!SyntaxKind::CLASS_KW.is_expression_keyword());
    assert!(!SyntaxKind::IF_KW.is_expression_keyword());
    assert!(!SyntaxKind::WHILE_KW.is_expression_keyword());
    assert!(!SyntaxKind::PUBLIC_KW.is_expression_keyword());
}

// ============================================================================
// SyntaxKind String Representation Tests
// ============================================================================

#[test]
fn test_keyword_as_str() {
    assert_eq!(SyntaxKind::CLASS_KW.as_str(), "class");
    assert_eq!(SyntaxKind::PUBLIC_KW.as_str(), "public");
    assert_eq!(SyntaxKind::STATIC_KW.as_str(), "static");
    assert_eq!(SyntaxKind::VOID_KW.as_str(), "void");
}

#[test]
fn test_operator_as_str() {
    assert_eq!(SyntaxKind::PLUS.as_str(), "+");
    assert_eq!(SyntaxKind::MINUS.as_str(), "-");
    assert_eq!(SyntaxKind::STAR.as_str(), "*");
    assert_eq!(SyntaxKind::EQ_EQ.as_str(), "==");
    assert_eq!(SyntaxKind::ARROW.as_str(), "->");
    assert_eq!(SyntaxKind::COLON_COLON.as_str(), "::");
}

#[test]
fn test_punctuation_as_str() {
    assert_eq!(SyntaxKind::L_PAREN.as_str(), "(");
    assert_eq!(SyntaxKind::R_PAREN.as_str(), ")");
    assert_eq!(SyntaxKind::L_BRACE.as_str(), "{");
    assert_eq!(SyntaxKind::R_BRACE.as_str(), "}");
    assert_eq!(SyntaxKind::SEMICOLON.as_str(), ";");
}

#[test]
fn test_literal_as_str() {
    assert_eq!(SyntaxKind::INT_LITERAL.as_str(), "INT_LITERAL");
    assert_eq!(SyntaxKind::STRING_LITERAL.as_str(), "STRING_LITERAL");
    assert_eq!(SyntaxKind::BOOLEAN_LITERAL.as_str(), "BOOLEAN_LITERAL");
}

#[test]
fn test_node_as_str() {
    assert_eq!(SyntaxKind::SOURCE_FILE.as_str(), "SOURCE_FILE");
    assert_eq!(SyntaxKind::CLASS_DECL.as_str(), "CLASS_DECL");
    assert_eq!(SyntaxKind::METHOD_DECL.as_str(), "METHOD_DECL");
}

#[test]
fn test_display_trait() {
    assert_eq!(format!("{}", SyntaxKind::CLASS_KW), "class");
    assert_eq!(format!("{}", SyntaxKind::PLUS), "+");
    assert_eq!(format!("{}", SyntaxKind::SOURCE_FILE), "SOURCE_FILE");
}

// ============================================================================
// Edge Case and Boundary Tests
// ============================================================================

#[test]
fn test_eof_token() {
    assert!(!SyntaxKind::EOF.is_keyword());
    assert!(!SyntaxKind::EOF.is_literal());
    assert!(!SyntaxKind::EOF.is_operator());
    assert!(!SyntaxKind::EOF.is_punctuation());
    assert!(!SyntaxKind::EOF.is_trivia());
    assert!(!SyntaxKind::EOF.is_node());
}

#[test]
fn test_error_token() {
    assert!(!SyntaxKind::ERROR.is_keyword());
    assert!(!SyntaxKind::ERROR.is_literal());
    assert!(!SyntaxKind::ERROR.is_operator());
    assert!(!SyntaxKind::ERROR.is_punctuation());
    assert!(!SyntaxKind::ERROR.is_trivia());
}

#[test]
fn test_tombstone_token() {
    assert!(!SyntaxKind::TOMBSTONE.is_keyword());
    assert!(!SyntaxKind::TOMBSTONE.is_literal());
    assert!(!SyntaxKind::TOMBSTONE.is_operator());
    assert!(!SyntaxKind::TOMBSTONE.is_punctuation());
    assert!(!SyntaxKind::TOMBSTONE.is_trivia());
}

// ============================================================================
// Comprehensive Classification Tests
// ============================================================================

#[test]
fn test_every_keyword_is_classified() {
    // Every keyword should return true for is_keyword()
    let keywords = vec![
        SyntaxKind::ABSTRACT_KW,
        SyntaxKind::ASSERT_KW,
        SyntaxKind::BOOLEAN_KW,
        SyntaxKind::BREAK_KW,
        SyntaxKind::BYTE_KW,
        SyntaxKind::CASE_KW,
        SyntaxKind::CATCH_KW,
        SyntaxKind::CHAR_KW,
        SyntaxKind::CLASS_KW,
        SyntaxKind::CONST_KW,
        SyntaxKind::CONTINUE_KW,
        SyntaxKind::DEFAULT_KW,
        SyntaxKind::DO_KW,
        SyntaxKind::DOUBLE_KW,
        SyntaxKind::ELSE_KW,
        SyntaxKind::ENUM_KW,
        SyntaxKind::EXTENDS_KW,
        SyntaxKind::FINAL_KW,
        SyntaxKind::FINALLY_KW,
        SyntaxKind::FLOAT_KW,
        SyntaxKind::FOR_KW,
        SyntaxKind::GOTO_KW,
        SyntaxKind::IF_KW,
        SyntaxKind::IMPLEMENTS_KW,
        SyntaxKind::IMPORT_KW,
        SyntaxKind::INSTANCEOF_KW,
        SyntaxKind::INT_KW,
        SyntaxKind::INTERFACE_KW,
        SyntaxKind::LONG_KW,
        SyntaxKind::NATIVE_KW,
        SyntaxKind::NEW_KW,
        SyntaxKind::PACKAGE_KW,
        SyntaxKind::PRIVATE_KW,
        SyntaxKind::PROTECTED_KW,
        SyntaxKind::PUBLIC_KW,
        SyntaxKind::RETURN_KW,
        SyntaxKind::SHORT_KW,
        SyntaxKind::STATIC_KW,
        SyntaxKind::STRICTFP_KW,
        SyntaxKind::SUPER_KW,
        SyntaxKind::SWITCH_KW,
        SyntaxKind::SYNCHRONIZED_KW,
        SyntaxKind::THIS_KW,
        SyntaxKind::THROW_KW,
        SyntaxKind::THROWS_KW,
        SyntaxKind::TRANSIENT_KW,
        SyntaxKind::TRY_KW,
        SyntaxKind::VOID_KW,
        SyntaxKind::VOLATILE_KW,
        SyntaxKind::WHILE_KW,
        // Contextual keywords
        SyntaxKind::MODULE_KW,
        SyntaxKind::REQUIRES_KW,
        SyntaxKind::EXPORTS_KW,
        SyntaxKind::OPENS_KW,
        SyntaxKind::PROVIDES_KW,
        SyntaxKind::USES_KW,
        SyntaxKind::TO_KW,
        SyntaxKind::WITH_KW,
        SyntaxKind::TRANSITIVE_KW,
        SyntaxKind::OPEN_KW,
        SyntaxKind::RECORD_KW,
        SyntaxKind::SEALED_KW,
        SyntaxKind::PERMITS_KW,
        SyntaxKind::NON_SEALED_KW,
        SyntaxKind::VAR_KW,
        SyntaxKind::YIELD_KW,
    ];

    for kw in keywords {
        assert!(kw.is_keyword(), "{kw:?} should be classified as keyword");
    }
}

#[test]
fn test_every_literal_is_classified() {
    let literals = vec![
        SyntaxKind::INT_LITERAL,
        SyntaxKind::LONG_LITERAL,
        SyntaxKind::FLOAT_LITERAL,
        SyntaxKind::DOUBLE_LITERAL,
        SyntaxKind::CHAR_LITERAL,
        SyntaxKind::STRING_LITERAL,
        SyntaxKind::TEXT_BLOCK_LITERAL,
        SyntaxKind::BOOLEAN_LITERAL,
        SyntaxKind::NULL_LITERAL,
    ];

    for lit in literals {
        assert!(lit.is_literal(), "{lit:?} should be classified as literal");
    }
}

// ============================================================================
// Java Version Specific Tests
// ============================================================================

#[test]
fn test_java8_keywords() {
    // Lambda and method reference aren't keywords, but operators
    assert!(SyntaxKind::ARROW.is_operator());
    assert!(SyntaxKind::COLON_COLON.is_operator());
}

#[test]
fn test_java9_module_keywords() {
    assert!(SyntaxKind::MODULE_KW.is_keyword());
    assert!(SyntaxKind::REQUIRES_KW.is_keyword());
    assert!(SyntaxKind::EXPORTS_KW.is_keyword());
    assert!(SyntaxKind::OPENS_KW.is_keyword());
    assert!(SyntaxKind::PROVIDES_KW.is_keyword());
    assert!(SyntaxKind::USES_KW.is_keyword());
    assert!(SyntaxKind::TO_KW.is_keyword());
    assert!(SyntaxKind::WITH_KW.is_keyword());
    assert!(SyntaxKind::TRANSITIVE_KW.is_keyword());
}

#[test]
fn test_java10_var_keyword() {
    assert!(SyntaxKind::VAR_KW.is_keyword());
}

#[test]
fn test_java13_yield_keyword() {
    assert!(SyntaxKind::YIELD_KW.is_keyword());
    assert!(SyntaxKind::YIELD_KW.is_statement_keyword());
}

#[test]
fn test_java14_record_keyword() {
    assert!(SyntaxKind::RECORD_KW.is_keyword());
}

#[test]
fn test_java15_sealed_keywords() {
    assert!(SyntaxKind::SEALED_KW.is_keyword());
    assert!(SyntaxKind::PERMITS_KW.is_keyword());
    assert!(SyntaxKind::NON_SEALED_KW.is_keyword());

    assert!(SyntaxKind::SEALED_KW.is_modifier());
    assert!(SyntaxKind::NON_SEALED_KW.is_modifier());
}
