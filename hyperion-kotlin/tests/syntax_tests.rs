// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Tests for Kotlin Syntax Tree and `SyntaxKind`
//!
//! Tests the syntax kind classification and rowan integration.

use hyperion_kotlin::syntax::{KotlinLanguage, SyntaxKind};
use rowan::Language;

#[test]
fn test_syntax_kind_is_trivia() {
    assert!(SyntaxKind::WHITESPACE.is_trivia());
    assert!(SyntaxKind::LINE_COMMENT.is_trivia());
    assert!(SyntaxKind::BLOCK_COMMENT.is_trivia());

    assert!(!SyntaxKind::FUN_KW.is_trivia());
    assert!(!SyntaxKind::IDENT.is_trivia());
    assert!(!SyntaxKind::INT_LITERAL.is_trivia());
}

#[test]
fn test_syntax_kind_is_keyword() {
    // Keywords
    assert!(SyntaxKind::FUN_KW.is_keyword());
    assert!(SyntaxKind::VAL_KW.is_keyword());
    assert!(SyntaxKind::VAR_KW.is_keyword());
    assert!(SyntaxKind::CLASS_KW.is_keyword());
    assert!(SyntaxKind::OBJECT_KW.is_keyword());
    assert!(SyntaxKind::INTERFACE_KW.is_keyword());
    assert!(SyntaxKind::IF_KW.is_keyword());
    assert!(SyntaxKind::WHEN_KW.is_keyword());
    assert!(SyntaxKind::FOR_KW.is_keyword());
    assert!(SyntaxKind::WHILE_KW.is_keyword());

    // Not keywords
    assert!(!SyntaxKind::IDENT.is_keyword());
    assert!(!SyntaxKind::INT_LITERAL.is_keyword());
    assert!(!SyntaxKind::PLUS.is_keyword());
    assert!(!SyntaxKind::L_BRACE.is_keyword());
}

#[test]
fn test_syntax_kind_is_literal() {
    // Literals
    assert!(SyntaxKind::INT_LITERAL.is_literal());
    assert!(SyntaxKind::LONG_LITERAL.is_literal());
    assert!(SyntaxKind::FLOAT_LITERAL.is_literal());
    assert!(SyntaxKind::DOUBLE_LITERAL.is_literal());
    assert!(SyntaxKind::STRING_LITERAL.is_literal());
    assert!(SyntaxKind::CHAR_LITERAL.is_literal());
    assert!(SyntaxKind::RAW_STRING_LITERAL.is_literal());
    assert!(SyntaxKind::TRUE_KW.is_literal());
    assert!(SyntaxKind::FALSE_KW.is_literal());
    assert!(SyntaxKind::NULL_KW.is_literal());

    // Not literals
    assert!(!SyntaxKind::IDENT.is_literal());
    assert!(!SyntaxKind::PLUS.is_literal());
    assert!(!SyntaxKind::FUN_KW.is_literal());
}

#[test]
fn test_all_keywords_are_keywords() {
    let keywords = vec![
        SyntaxKind::FUN_KW,
        SyntaxKind::VAL_KW,
        SyntaxKind::VAR_KW,
        SyntaxKind::CLASS_KW,
        SyntaxKind::OBJECT_KW,
        SyntaxKind::INTERFACE_KW,
        SyntaxKind::IF_KW,
        SyntaxKind::ELSE_KW,
        SyntaxKind::WHEN_KW,
        SyntaxKind::FOR_KW,
        SyntaxKind::WHILE_KW,
        SyntaxKind::RETURN_KW,
        SyntaxKind::PACKAGE_KW,
        SyntaxKind::IMPORT_KW,
        SyntaxKind::AS_KW,
        SyntaxKind::IS_KW,
        SyntaxKind::IN_KW,
        SyntaxKind::OUT_KW,
        SyntaxKind::OVERRIDE_KW,
        SyntaxKind::OPEN_KW,
        SyntaxKind::ABSTRACT_KW,
        SyntaxKind::DATA_KW,
        SyntaxKind::SEALED_KW,
        SyntaxKind::INTERNAL_KW,
        SyntaxKind::PRIVATE_KW,
        SyntaxKind::PUBLIC_KW,
        SyntaxKind::PROTECTED_KW,
        SyntaxKind::SUSPEND_KW,
        SyntaxKind::INLINE_KW,
        SyntaxKind::OPERATOR_KW,
        SyntaxKind::INFIX_KW,
        SyntaxKind::COMPANION_KW,
        SyntaxKind::INIT_KW,
        SyntaxKind::CONSTRUCTOR_KW,
        SyntaxKind::THIS_KW,
        SyntaxKind::SUPER_KW,
        SyntaxKind::TRY_KW,
        SyntaxKind::CATCH_KW,
        SyntaxKind::FINALLY_KW,
        SyntaxKind::THROW_KW,
        SyntaxKind::BREAK_KW,
        SyntaxKind::CONTINUE_KW,
    ];

    for kw in keywords {
        assert!(kw.is_keyword(), "{kw:?} should be a keyword");
    }
}

#[test]
fn test_trivia_not_keywords() {
    assert!(!SyntaxKind::WHITESPACE.is_keyword());
    assert!(!SyntaxKind::LINE_COMMENT.is_keyword());
    assert!(!SyntaxKind::BLOCK_COMMENT.is_keyword());
}

#[test]
fn test_trivia_not_literals() {
    assert!(!SyntaxKind::WHITESPACE.is_literal());
    assert!(!SyntaxKind::LINE_COMMENT.is_literal());
    assert!(!SyntaxKind::BLOCK_COMMENT.is_literal());
}

#[test]
fn test_kotlin_language_kind_conversion() {
    let kinds = vec![
        SyntaxKind::FUN_KW,
        SyntaxKind::CLASS_KW,
        SyntaxKind::IDENT,
        SyntaxKind::INT_LITERAL,
        SyntaxKind::PLUS,
        SyntaxKind::L_BRACE,
    ];

    for kind in kinds {
        let raw = KotlinLanguage::kind_to_raw(kind);
        let back = KotlinLanguage::kind_from_raw(raw);
        assert_eq!(kind, back);
    }
}

#[test]
fn test_syntax_kind_u16_conversion() {
    // Test conversion to/from u16
    let kinds = vec![
        SyntaxKind::WHITESPACE,
        SyntaxKind::FUN_KW,
        SyntaxKind::IDENT,
        SyntaxKind::INT_LITERAL,
        SyntaxKind::SOURCE_FILE,
        SyntaxKind::ERROR,
        SyntaxKind::EOF,
    ];

    for kind in kinds {
        let as_u16: u16 = kind.into();
        let back: SyntaxKind = as_u16.into();
        assert_eq!(kind, back);
    }
}

#[test]
fn test_syntax_kind_display() {
    let kind = SyntaxKind::FUN_KW;
    let display = format!("{kind}");
    assert!(!display.is_empty());
}

#[test]
fn test_syntax_kind_debug() {
    let kind = SyntaxKind::CLASS_KW;
    let debug = format!("{kind:?}");
    assert!(!debug.is_empty());
}

mod operator_classification {
    use super::*;

    #[test]
    fn test_arithmetic_operators() {
        let ops = vec![
            SyntaxKind::PLUS,
            SyntaxKind::MINUS,
            SyntaxKind::STAR,
            SyntaxKind::SLASH,
            SyntaxKind::PERCENT,
        ];

        for op in ops {
            assert!(!op.is_keyword());
            assert!(!op.is_literal());
            assert!(!op.is_trivia());
        }
    }

    #[test]
    fn test_kotlin_specific_operators() {
        let ops = vec![
            SyntaxKind::ARROW,
            SyntaxKind::ELVIS,
            SyntaxKind::SAFE_CALL,
            SyntaxKind::NOT_NULL,
            SyntaxKind::RANGE,
            SyntaxKind::RANGE_UNTIL,
            SyntaxKind::COLONCOLON,
        ];

        for op in ops {
            assert!(!op.is_keyword());
            assert!(!op.is_literal());
            assert!(!op.is_trivia());
        }
    }
}

mod node_kind_tests {
    use super::*;

    #[test]
    fn test_composite_nodes_not_trivia() {
        let nodes = vec![
            SyntaxKind::SOURCE_FILE,
            SyntaxKind::FUNCTION_DECL,
            SyntaxKind::CLASS_DECL,
            SyntaxKind::PROPERTY_DECL,
            SyntaxKind::LAMBDA_EXPR,
        ];

        for node in nodes {
            assert!(!node.is_trivia());
        }
    }

    #[test]
    fn test_composite_nodes_not_keywords() {
        let nodes = vec![
            SyntaxKind::SOURCE_FILE,
            SyntaxKind::FUNCTION_DECL,
            SyntaxKind::CLASS_DECL,
        ];

        for node in nodes {
            assert!(!node.is_keyword());
        }
    }

    #[test]
    fn test_composite_nodes_not_literals() {
        let nodes = vec![
            SyntaxKind::SOURCE_FILE,
            SyntaxKind::FUNCTION_DECL,
            SyntaxKind::CLASS_DECL,
        ];

        for node in nodes {
            assert!(!node.is_literal());
        }
    }
}

mod delimiter_tests {
    use super::*;

    #[test]
    fn test_delimiters_not_keywords() {
        let delims = vec![
            SyntaxKind::L_PAREN,
            SyntaxKind::R_PAREN,
            SyntaxKind::L_BRACE,
            SyntaxKind::R_BRACE,
            SyntaxKind::L_BRACKET,
            SyntaxKind::R_BRACKET,
        ];

        for delim in delims {
            assert!(!delim.is_keyword());
        }
    }

    #[test]
    fn test_punctuation_not_keywords() {
        let puncts = vec![
            SyntaxKind::SEMICOLON,
            SyntaxKind::COLON,
            SyntaxKind::COMMA,
            SyntaxKind::DOT,
        ];

        for punct in puncts {
            assert!(!punct.is_keyword());
        }
    }
}

mod special_kinds {
    use super::*;

    #[test]
    fn test_error_kind() {
        assert!(!SyntaxKind::ERROR.is_keyword());
        assert!(!SyntaxKind::ERROR.is_literal());
        assert!(!SyntaxKind::ERROR.is_trivia());
    }

    #[test]
    fn test_eof_kind() {
        assert!(!SyntaxKind::EOF.is_keyword());
        assert!(!SyntaxKind::EOF.is_literal());
        assert!(!SyntaxKind::EOF.is_trivia());
    }

    #[test]
    fn test_tombstone_kind() {
        assert!(!SyntaxKind::TOMBSTONE.is_keyword());
        assert!(!SyntaxKind::TOMBSTONE.is_literal());
        assert!(!SyntaxKind::TOMBSTONE.is_trivia());
    }
}

mod literal_classification {
    use super::*;

    #[test]
    fn test_all_numeric_literals() {
        let literals = vec![
            SyntaxKind::INT_LITERAL,
            SyntaxKind::LONG_LITERAL,
            SyntaxKind::FLOAT_LITERAL,
            SyntaxKind::DOUBLE_LITERAL,
        ];

        for lit in literals {
            assert!(lit.is_literal());
        }
    }

    #[test]
    fn test_all_string_literals() {
        let literals = vec![
            SyntaxKind::STRING_LITERAL,
            SyntaxKind::CHAR_LITERAL,
            SyntaxKind::RAW_STRING_LITERAL,
        ];

        for lit in literals {
            assert!(lit.is_literal());
        }
    }

    #[test]
    fn test_boolean_and_null_literals() {
        let literals = vec![
            SyntaxKind::TRUE_KW,
            SyntaxKind::FALSE_KW,
            SyntaxKind::NULL_KW,
        ];

        for lit in literals {
            assert!(lit.is_literal());
            // These are also keywords
            assert!(lit.is_keyword());
        }
    }
}

mod rowan_integration {
    use super::*;
    use rowan::GreenNodeBuilder;

    #[test]
    fn test_can_build_green_tree() {
        let mut builder = GreenNodeBuilder::new();

        builder.start_node(KotlinLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE));
        builder.token(KotlinLanguage::kind_to_raw(SyntaxKind::FUN_KW), "fun");
        builder.token(KotlinLanguage::kind_to_raw(SyntaxKind::WHITESPACE), " ");
        builder.token(KotlinLanguage::kind_to_raw(SyntaxKind::IDENT), "main");
        builder.finish_node();

        let green = builder.finish();
        assert_eq!(
            green.kind(),
            KotlinLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE)
        );
    }

    #[test]
    fn test_syntax_node_creation() {
        use hyperion_kotlin::syntax::SyntaxNode;
        use rowan::GreenNodeBuilder;

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(KotlinLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE));
        builder.token(KotlinLanguage::kind_to_raw(SyntaxKind::IDENT), "test");
        builder.finish_node();

        let green = builder.finish();
        let node = SyntaxNode::new_root(green);

        assert_eq!(node.kind(), SyntaxKind::SOURCE_FILE);
    }
}

mod coverage_tests {
    use super::*;

    #[test]
    fn test_all_expression_kinds() {
        let exprs = vec![
            SyntaxKind::BINARY_EXPR,
            SyntaxKind::UNARY_EXPR,
            SyntaxKind::LAMBDA_EXPR,
            SyntaxKind::CALL_EXPR,
            SyntaxKind::IF_EXPR,
            SyntaxKind::WHEN_EXPR,
            SyntaxKind::RETURN_EXPR,
            SyntaxKind::THROW_EXPR,
            SyntaxKind::TRY_EXPR,
        ];

        for expr in exprs {
            assert!(!expr.is_keyword());
            assert!(!expr.is_literal());
            assert!(!expr.is_trivia());
        }
    }

    #[test]
    fn test_all_declaration_kinds() {
        let decls = vec![
            SyntaxKind::FUNCTION_DECL,
            SyntaxKind::PROPERTY_DECL,
            SyntaxKind::CLASS_DECL,
            SyntaxKind::OBJECT_DECL,
            SyntaxKind::INTERFACE_DECL,
            SyntaxKind::PACKAGE_DECL,
            SyntaxKind::IMPORT_DECL,
        ];

        for decl in decls {
            assert!(!decl.is_keyword());
            assert!(!decl.is_literal());
            assert!(!decl.is_trivia());
        }
    }
}

mod equality_tests {
    use super::*;

    #[test]
    fn test_syntax_kind_equality() {
        assert_eq!(SyntaxKind::FUN_KW, SyntaxKind::FUN_KW);
        assert_ne!(SyntaxKind::FUN_KW, SyntaxKind::VAL_KW);
    }

    #[test]
    fn test_syntax_kind_clone() {
        let kind = SyntaxKind::CLASS_KW;
        let cloned = kind;
        assert_eq!(kind, cloned);
    }

    #[test]
    fn test_syntax_kind_copy() {
        let kind = SyntaxKind::IDENT;
        let copied = kind;
        assert_eq!(kind, copied);
    }
}

mod ordering_tests {
    use super::*;

    #[test]
    fn test_syntax_kind_ordering() {
        // SyntaxKind is repr(u16) with sequential values
        assert!(SyntaxKind::WHITESPACE < SyntaxKind::LINE_COMMENT);
        assert!(SyntaxKind::FUN_KW < SyntaxKind::VAL_KW);
    }

    #[test]
    fn test_syntax_kind_ord() {
        use std::cmp::Ordering;

        let kind1 = SyntaxKind::FUN_KW;
        let kind2 = SyntaxKind::CLASS_KW;

        let ord = kind1.cmp(&kind2);
        assert!(matches!(ord, Ordering::Less | Ordering::Greater));
    }
}

mod hash_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_syntax_kind_hashable() {
        let mut set = HashSet::new();
        set.insert(SyntaxKind::FUN_KW);
        set.insert(SyntaxKind::CLASS_KW);
        set.insert(SyntaxKind::IDENT);

        assert_eq!(set.len(), 3);
        assert!(set.contains(&SyntaxKind::FUN_KW));
    }

    #[test]
    fn test_syntax_kind_hash_equality() {
        let mut set1 = HashSet::new();
        set1.insert(SyntaxKind::VAL_KW);

        let mut set2 = HashSet::new();
        set2.insert(SyntaxKind::VAL_KW);

        assert_eq!(set1, set2);
    }
}
