// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Identifier expression type inference
//!
//! Handles type inference for identifiers (variable names, field names, parameter names).
//! Identifiers are resolved through the SymbolResolver to find their declared types.

use crate::inference::visitor::InferType;
use crate::inference::TypeInferenceEngine;
use crate::resolver::SymbolResolver;
use crate::symbol::Type;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use tracing::debug;

/// Type-safe wrapper for identifier expressions
///
/// Represents a reference to a named entity:
/// - Local variables: `count`, `user`, `result`
/// - Parameters: `request`, `data`
/// - Fields: `this.name`, `user.email` (the `name` and `email` parts)
/// - Class names: `String`, `MyClass`
///
/// # Example
///
/// ```java
/// void process(User user) {  // `user` is an identifier
///     String name = user.name;  // `name` is an identifier
///     int count = 0;  // `count` is an identifier
///     System.out.println(count);  // `count` is an identifier
/// }
/// ```
pub struct IdentifierExpr<'a> {
    node: &'a SyntaxNode,
}

impl<'a> IdentifierExpr<'a> {
    /// Try to create an identifier expression from a syntax node
    ///
    /// Returns `None` if the node is not an identifier.
    pub fn cast(node: &'a SyntaxNode) -> Option<Self> {
        if node.kind() == SyntaxKind::IDENT {
            Some(Self { node })
        } else {
            None
        }
    }

    /// Get the identifier name
    pub fn name(&self) -> String {
        self.node.text().to_string()
    }
}

impl InferType for IdentifierExpr<'_> {
    fn infer_type(
        &self,
        _engine: &TypeInferenceEngine,
        resolver: &SymbolResolver,
        _expected_type: Option<&Type>,
    ) -> Type {
        let name = self.name();
        debug!("Inferring identifier type: {}", name);

        // Look up the identifier in the resolver
        if let Some(symbol) = resolver.resolve_identifier(&name, self.node) {
            debug!("Resolved {} to type {:?}", name, symbol.type_info);
            return symbol.type_info;
        }

        debug!("Could not resolve identifier: {}", name);
        Type::Unknown
    }
}
