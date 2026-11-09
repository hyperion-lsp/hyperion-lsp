// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Token Classification Utilities
//!
//! This module provides helper functions for classifying and working with tokens.
//! Note: The actual token definitions live in `syntax.rs` as `SyntaxKind`.
//! This module just provides convenient classification helpers.

use crate::syntax::SyntaxKind;

/// Extension trait for `SyntaxKind` to provide token classification methods
pub trait TokenClassification {
    /// Returns true if this is a modifier keyword (public, static, etc.)
    fn is_modifier(&self) -> bool;

    /// Returns true if this is a primitive type keyword
    fn is_primitive_type(&self) -> bool;

    /// Returns true if this is a statement keyword
    fn is_statement_keyword(&self) -> bool;

    /// Returns true if this is an expression keyword
    fn is_expression_keyword(&self) -> bool;
}

impl TokenClassification for SyntaxKind {
    fn is_modifier(&self) -> bool {
        matches!(
            self,
            Self::PUBLIC_KW
                | Self::PRIVATE_KW
                | Self::PROTECTED_KW
                | Self::STATIC_KW
                | Self::FINAL_KW
                | Self::ABSTRACT_KW
                | Self::NATIVE_KW
                | Self::SYNCHRONIZED_KW
                | Self::TRANSIENT_KW
                | Self::VOLATILE_KW
                | Self::STRICTFP_KW
                | Self::DEFAULT_KW
                | Self::SEALED_KW
                | Self::NON_SEALED_KW
        )
    }

    fn is_primitive_type(&self) -> bool {
        matches!(
            self,
            Self::BOOLEAN_KW
                | Self::BYTE_KW
                | Self::SHORT_KW
                | Self::INT_KW
                | Self::LONG_KW
                | Self::CHAR_KW
                | Self::FLOAT_KW
                | Self::DOUBLE_KW
                | Self::VOID_KW
        )
    }

    fn is_statement_keyword(&self) -> bool {
        matches!(
            self,
            Self::IF_KW
                | Self::ELSE_KW
                | Self::WHILE_KW
                | Self::DO_KW
                | Self::FOR_KW
                | Self::SWITCH_KW
                | Self::CASE_KW
                | Self::DEFAULT_KW
                | Self::BREAK_KW
                | Self::CONTINUE_KW
                | Self::RETURN_KW
                | Self::THROW_KW
                | Self::TRY_KW
                | Self::CATCH_KW
                | Self::FINALLY_KW
                | Self::ASSERT_KW
                | Self::SYNCHRONIZED_KW
                | Self::YIELD_KW
        )
    }

    fn is_expression_keyword(&self) -> bool {
        matches!(
            self,
            Self::NEW_KW | Self::THIS_KW | Self::SUPER_KW | Self::INSTANCEOF_KW
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_modifier() {
        assert!(SyntaxKind::PUBLIC_KW.is_modifier());
        assert!(SyntaxKind::STATIC_KW.is_modifier());
        assert!(SyntaxKind::FINAL_KW.is_modifier());
        assert!(!SyntaxKind::CLASS_KW.is_modifier());
        assert!(!SyntaxKind::IF_KW.is_modifier());
    }

    #[test]
    fn test_is_primitive_type() {
        assert!(SyntaxKind::INT_KW.is_primitive_type());
        assert!(SyntaxKind::BOOLEAN_KW.is_primitive_type());
        assert!(SyntaxKind::VOID_KW.is_primitive_type());
        assert!(!SyntaxKind::CLASS_KW.is_primitive_type());
    }

    #[test]
    fn test_is_statement_keyword() {
        assert!(SyntaxKind::IF_KW.is_statement_keyword());
        assert!(SyntaxKind::WHILE_KW.is_statement_keyword());
        assert!(SyntaxKind::RETURN_KW.is_statement_keyword());
        assert!(!SyntaxKind::CLASS_KW.is_statement_keyword());
    }

    #[test]
    fn test_is_expression_keyword() {
        assert!(SyntaxKind::NEW_KW.is_expression_keyword());
        assert!(SyntaxKind::THIS_KW.is_expression_keyword());
        assert!(SyntaxKind::INSTANCEOF_KW.is_expression_keyword());
        assert!(!SyntaxKind::CLASS_KW.is_expression_keyword());
    }
}
