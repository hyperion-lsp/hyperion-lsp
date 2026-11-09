// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Navigation utility functions

use hyperion_parser::syntax::SyntaxNode;
use hyperion_semantics::resolver::SymbolResolver;
use hyperion_semantics::symbol::{ClassType, ScopeId, Type};
use tower_lsp::lsp_types::Position;

/// Find the scope at a given byte offset
///
/// This uses the symbol table's scope range tracking to find the deepest scope
/// that contains the given offset.
pub fn find_scope_at_position(
    _root: &SyntaxNode,
    offset: u32,
    resolver: &SymbolResolver,
) -> Option<ScopeId> {
    resolver.find_scope_at_offset(offset)
}

/// Convert LSP Position to byte offset
///
/// # Arguments
///
/// * `source` - The source text
/// * `position` - The LSP position (line, character in UTF-16 units)
///
/// # Returns
///
/// The byte offset corresponding to the position
pub fn position_to_offset(
    source: &str,
    position: Position,
) -> Result<u32, crate::position_utils::PositionError> {
    crate::position_utils::position_to_offset_u32(source, position)
}

/// Convert byte offset to LSP Position
///
/// # Arguments
///
/// * `source` - The source text
/// * `offset` - The byte offset
///
/// # Returns
///
/// The LSP Position (line, character in UTF-16 units)
pub fn offset_to_position(source: &str, offset: usize) -> Position {
    crate::position_utils::offset_to_position(source, offset)
}

/// Convert a fully-qualified name string to a `Type::Class`.
///
/// Splits on the last `.` to extract package and simple name.
/// Handles edge cases like no-dot names and malformed FQNs without panicking.
///
/// # Examples
/// - `"com.example.User"` → `Type::Class { name: "User", package: Some("com.example"), .. }`
/// - `"User"` → `Type::Class { name: "User", package: None, .. }`
pub fn fqn_to_type(fqn: &str) -> Type {
    match fqn.rfind('.') {
        Some(dot_idx) if dot_idx + 1 < fqn.len() => {
            let (pkg, name_with_dot) = fqn.split_at(dot_idx);
            Type::Class(ClassType {
                name: name_with_dot[1..].to_string(),
                package: Some(pkg.to_string()),
                type_arguments: vec![],
                is_diamond: false,
            })
        }
        _ => Type::Class(ClassType {
            name: fqn.to_string(),
            package: None,
            type_arguments: vec![],
            is_diamond: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fqn_to_type_with_package() {
        let ty = fqn_to_type("com.example.User");
        if let Type::Class(ct) = ty {
            assert_eq!(ct.name, "User");
            assert_eq!(ct.package, Some("com.example".to_string()));
            assert!(ct.type_arguments.is_empty());
            assert!(!ct.is_diamond);
        } else {
            panic!("Expected Type::Class");
        }
    }

    #[test]
    fn fqn_to_type_simple_name() {
        let ty = fqn_to_type("User");
        if let Type::Class(ct) = ty {
            assert_eq!(ct.name, "User");
            assert_eq!(ct.package, None);
        } else {
            panic!("Expected Type::Class");
        }
    }

    #[test]
    fn fqn_to_type_deep_package() {
        let ty = fqn_to_type("com.example.deep.pkg.Thing");
        if let Type::Class(ct) = ty {
            assert_eq!(ct.name, "Thing");
            assert_eq!(ct.package, Some("com.example.deep.pkg".to_string()));
        } else {
            panic!("Expected Type::Class");
        }
    }

    #[test]
    fn fqn_to_type_trailing_dot() {
        // Edge case: trailing dot should produce no-package type with the whole string
        let ty = fqn_to_type("com.example.");
        if let Type::Class(ct) = ty {
            // Last dot with nothing after → falls to default branch
            assert_eq!(ct.name, "com.example.");
            assert_eq!(ct.package, None);
        } else {
            panic!("Expected Type::Class");
        }
    }

    #[test]
    fn fqn_to_type_empty_string() {
        let ty = fqn_to_type("");
        if let Type::Class(ct) = ty {
            assert_eq!(ct.name, "");
            assert_eq!(ct.package, None);
        } else {
            panic!("Expected Type::Class");
        }
    }
}
