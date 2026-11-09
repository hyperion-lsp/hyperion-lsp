// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type heuristics for security scanning
//!
//! These checks reduce false positives in security analysis. They look at the
//! shape of the syntax tree and at receiver names. They do not query
//! hyperion-semantics, so they cannot see the declared type of an expression.

use hyperion_parser::syntax::{AstNode, MethodCallExpr, SyntaxNode};
use hyperion_semantics::Type;

/// Syntactic type heuristics used by the security scanner
#[derive(Debug, Clone, Copy, Default)]
pub struct TypeAwareAnalyzer;

impl TypeAwareAnalyzer {
    /// Create a new analyzer
    pub const fn new() -> Self {
        Self
    }

    /// Check if a method call returns a numeric type
    ///
    /// Returns true if the method is known to return int, long, short, byte, float, double
    /// or their boxed equivalents (Integer, Long, etc.)
    pub fn returns_numeric_type(&self, method_call: &MethodCallExpr) -> bool {
        // The database is accepted for API compatibility, but semantics does
        // not yet expose a method-return query. Until it does, this function's
        // contract is the explicit conversion-method allowlist below.
        // Fallback to pattern matching for common numeric conversion methods
        Self::is_numeric_conversion_method(method_call)
    }

    /// Check if a method call is a known numeric conversion method
    ///
    /// Detects patterns like:
    /// - Integer.parseInt(...)
    /// - Long.parseLong(...)
    /// - Integer.valueOf(...)
    /// - BigInteger/BigDecimal conversions
    ///
    /// etc.
    fn is_numeric_conversion_method(method_call: &MethodCallExpr) -> bool {
        let method_name = method_call.name().map(|t| t.text().to_string());

        let receiver_name = method_call
            .receiver()
            .and_then(|r| Self::get_receiver_name(r.syntax()));

        matches!(
            (receiver_name.as_deref(), method_name.as_deref()),
            (
                Some("Integer"),
                Some("parseInt" | "valueOf" | "parseUnsignedInt" | "decode")
            ) | (
                Some("Long"),
                Some("parseLong" | "valueOf" | "parseUnsignedLong" | "decode")
            ) | (Some("Short"), Some("parseShort" | "valueOf" | "decode"))
                | (Some("Byte"), Some("parseByte" | "valueOf" | "decode"))
                | (Some("Float"), Some("parseFloat" | "valueOf"))
                | (Some("Double"), Some("parseDouble" | "valueOf"))
                | (Some("BigInteger" | "BigDecimal"), Some("valueOf" | "new"))
                | (Some("Math"), Some(_))
        )
    }

    /// Check if an expression type is safe for SQL injection
    ///
    /// Numeric types are generally safe because they cannot contain SQL syntax.
    /// This helps eliminate false positives when numeric parameters are used.
    pub fn is_sql_safe_type(&self, node: &SyntaxNode) -> bool {
        // If this is a method call that returns numeric, it's safe
        if let Some(method_call) = MethodCallExpr::cast(node.clone()) {
            if self.returns_numeric_type(&method_call) {
                return true;
            }
        }

        // General expression types require a semantics query that is not yet
        // available. The database field is therefore not consulted here.
        false
    }

    /// Check if a method call creates a PreparedStatement
    ///
    /// PreparedStatements with proper parameterization are safe from SQL injection
    pub fn creates_prepared_statement(&self, method_call: &MethodCallExpr) -> bool {
        let method_name = method_call.name().map(|t| t.text().to_string());

        matches!(
            method_name.as_deref(),
            Some("prepareStatement" | "prepareCall")
        )
    }

    /// Check if a method call is a PreparedStatement setter (setString, setInt, etc.)
    ///
    /// These methods properly parameterize queries and prevent SQL injection
    pub fn is_prepared_statement_setter(&self, method_call: &MethodCallExpr) -> bool {
        let method_name = method_call.name().map(|t| t.text().to_string());

        match method_name.as_deref() {
            Some(name) if name.starts_with("set") => {
                // Common PreparedStatement setters
                matches!(
                    name,
                    "setString"
                        | "setInt"
                        | "setLong"
                        | "setDouble"
                        | "setFloat"
                        | "setBoolean"
                        | "setByte"
                        | "setShort"
                        | "setDate"
                        | "setTime"
                        | "setTimestamp"
                        | "setBytes"
                        | "setObject"
                        | "setBigDecimal"
                        | "setNull"
                        | "setBlob"
                        | "setClob"
                        | "setArray"
                        | "setRef"
                )
            }
            _ => false,
        }
    }

    /// Get the receiver name from a method call receiver expression
    fn get_receiver_name(node: &SyntaxNode) -> Option<String> {
        use hyperion_parser::syntax::{FieldAccessExpr, SyntaxKind};

        match node.kind() {
            SyntaxKind::NAME_EXPR => {
                // NameExpr doesn't have a name() method, manually extract IDENT token
                for child in node.children_with_tokens() {
                    if let Some(token) = child.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            return Some(token.text().to_string());
                        }
                    }
                }
                None
            }
            SyntaxKind::FIELD_ACCESS_EXPR => FieldAccessExpr::cast(node.clone())
                .and_then(|access| access.name().map(|t| t.text().to_string())),
            SyntaxKind::METHOD_CALL_EXPR => {
                // For chained calls, get the method name
                MethodCallExpr::cast(node.clone())
                    .and_then(|call| call.name().map(|t| t.text().to_string()))
            }
            _ => None,
        }
    }
}

/// Helper to determine if a Type from hyperion-semantics is numeric
pub fn is_numeric_type(ty: &Type) -> bool {
    match ty {
        Type::Primitive(prim) => {
            use hyperion_semantics::PrimitiveType;
            matches!(
                prim,
                PrimitiveType::Int
                    | PrimitiveType::Long
                    | PrimitiveType::Short
                    | PrimitiveType::Byte
                    | PrimitiveType::Float
                    | PrimitiveType::Double
            )
        }
        Type::Class(class) => {
            // Check for boxed numeric types
            matches!(
                class.name.as_ref(),
                "Integer" | "Long" | "Short" | "Byte" | "Float" | "Double" | "Number"
            )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    // Note: Due to the flat AST structure used by hyperion-parser,
    // MethodCallExpr nodes don't contain the method name as a child.
    // Integration tests are in scanner.rs where the full pattern matching happens.
}
