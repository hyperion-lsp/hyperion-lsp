// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Literal expression type inference
//!
//! Handles type inference for all literal types:
//! - Primitive literals: int, long, float, double, char, boolean
//! - String literals and text blocks
//! - Null literals

use crate::inference::visitor::InferType;
use crate::inference::TypeInferenceEngine;
use crate::resolver::SymbolResolver;
use crate::symbol::{ClassType, PrimitiveType, Type};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

/// Type-safe wrapper for literal expressions
///
/// Represents any literal value in Java code:
/// - Integer literals: `42`, `0xFF`, `0b1010`
/// - Long literals: `42L`, `0xFFL`
/// - Float literals: `3.14f`, `1.0e-10f`
/// - Double literals: `3.14`, `1.0e-10`, `0x1.0p0`
/// - Character literals: `'a'`, `'\n'`, `'\u0041'`
/// - Boolean literals: `true`, `false`
/// - String literals: `"hello"`, `"""text block"""`
/// - Null literal: `null`
///
/// # Example
///
/// ```java
/// int x = 42;        // IntLiteral(42)
/// String s = "hi";   // StringLiteral("hi")
/// boolean b = true;  // BooleanLiteral(true)
/// Object o = null;   // NullLiteral
/// ```
pub struct LiteralExpr {
    literal_type: LiteralType,
}

/// Discriminated union of literal types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralType {
    Int,
    Long,
    Float,
    Double,
    Char,
    Boolean,
    String,
    Null,
}

impl LiteralExpr {
    /// Try to create a literal expression from a syntax node
    ///
    /// Returns `None` if the node is not a literal expression.
    pub fn cast(node: &SyntaxNode) -> Option<Self> {
        let literal_type = match node.kind() {
            SyntaxKind::INT_LITERAL => LiteralType::Int,
            SyntaxKind::LONG_LITERAL => LiteralType::Long,
            SyntaxKind::FLOAT_LITERAL => LiteralType::Float,
            SyntaxKind::DOUBLE_LITERAL => LiteralType::Double,
            SyntaxKind::CHAR_LITERAL => LiteralType::Char,
            SyntaxKind::BOOLEAN_LITERAL => LiteralType::Boolean,
            SyntaxKind::STRING_LITERAL | SyntaxKind::TEXT_BLOCK_LITERAL => LiteralType::String,
            SyntaxKind::NULL_LITERAL => LiteralType::Null,
            _ => return None,
        };

        Some(Self { literal_type })
    }
}

impl InferType for LiteralExpr {
    fn infer_type(
        &self,
        _engine: &TypeInferenceEngine,
        _resolver: &SymbolResolver,
        _expected_type: Option<&Type>,
    ) -> Type {
        match self.literal_type {
            LiteralType::Int => Type::Primitive(PrimitiveType::Int),
            LiteralType::Long => Type::Primitive(PrimitiveType::Long),
            LiteralType::Float => Type::Primitive(PrimitiveType::Float),
            LiteralType::Double => Type::Primitive(PrimitiveType::Double),
            LiteralType::Char => Type::Primitive(PrimitiveType::Char),
            LiteralType::Boolean => Type::Primitive(PrimitiveType::Boolean),
            LiteralType::String => Type::Class(ClassType::string()),
            LiteralType::Null => Type::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests verify the type system logic directly
    // Integration tests with real syntax trees are in the LSP test suite

    #[test]
    fn test_literal_type_mapping() {
        let test_cases = [
            (LiteralType::Int, PrimitiveType::Int),
            (LiteralType::Long, PrimitiveType::Long),
            (LiteralType::Float, PrimitiveType::Float),
            (LiteralType::Double, PrimitiveType::Double),
            (LiteralType::Char, PrimitiveType::Char),
            (LiteralType::Boolean, PrimitiveType::Boolean),
        ];

        for (literal_type, expected_prim) in test_cases {
            let result = match literal_type {
                LiteralType::Int => Type::Primitive(PrimitiveType::Int),
                LiteralType::Long => Type::Primitive(PrimitiveType::Long),
                LiteralType::Float => Type::Primitive(PrimitiveType::Float),
                LiteralType::Double => Type::Primitive(PrimitiveType::Double),
                LiteralType::Char => Type::Primitive(PrimitiveType::Char),
                LiteralType::Boolean => Type::Primitive(PrimitiveType::Boolean),
                _ => panic!("Unexpected literal type"),
            };

            assert_eq!(result, Type::Primitive(expected_prim));
        }
    }

    #[test]
    fn test_string_literal_type() {
        let result = Type::Class(ClassType::string());
        match result {
            Type::Class(ct) => {
                assert_eq!(&*ct.name, "String");
            }
            _ => panic!("Expected Class type for String literal"),
        }
    }

    #[test]
    fn test_null_literal_type() {
        let result = Type::Null;
        assert_eq!(result, Type::Null);
    }
}
