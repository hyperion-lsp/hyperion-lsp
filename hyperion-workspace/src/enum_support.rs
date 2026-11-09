// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// ! Enum synthetic method generation
//!
//! Java enums automatically include several methods:
//! - `static T[] values()` - returns array of all enum constants
//! - `static T valueOf(String name)` - returns enum constant by name
//! - `String name()` - returns the name of the constant
//! - `int ordinal()` - returns the position of the constant
//!
//! This module generates synthetic symbols for these methods so they can be
//! navigated to the enum declaration.

use crate::{Modifiers, Symbol, TextRange};
use hyperion_parser::syntax::{SyntaxNode, TypeDecl};

/// Information about an enum extracted from AST
#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: String,
    pub constants: Vec<String>,
    pub range: TextRange,
}

impl EnumInfo {
    /// Extract enum info from an ENUM_DECL node
    pub fn from_node(node: &SyntaxNode) -> Option<Self> {
        // Cast to EnumDecl
        let type_decl = TypeDecl::cast(node.clone())?;
        let enum_decl = match type_decl {
            TypeDecl::Enum(e) => e,
            _ => return None,
        };

        // Get enum name
        let name = enum_decl.name()?.text().to_string();

        // Get enum constants
        let constants: Vec<String> = enum_decl
            .constants()
            .filter_map(|c| c.name().map(|n| n.text().to_string()))
            .collect();

        Some(EnumInfo {
            name,
            constants,
            range: TextRange::new(
                node.text_range().start().into(),
                node.text_range().end().into(),
            ),
        })
    }
}

/// Generate synthetic enum methods for an enum declaration
pub fn generate_enum_methods(
    _enum_info: &EnumInfo,
    enum_fqn: &str,
    enum_range: TextRange,
) -> Vec<Symbol> {
    vec![
        // Generate values() static method
        generate_values_method(enum_fqn, enum_range),
        // Generate valueOf(String) static method
        generate_valueof_method(enum_fqn, enum_range),
        // Generate name() instance method
        generate_name_method(enum_fqn, enum_range),
        // Generate ordinal() instance method
        generate_ordinal_method(enum_fqn, enum_range),
    ]
}

/// Generate `static T[] values()` method
fn generate_values_method(enum_fqn: &str, range: TextRange) -> Symbol {
    let enum_simple_name = enum_fqn.split('.').next_back().unwrap_or("");
    let return_type = format!("{}[]", enum_simple_name);

    Symbol {
        fqn: format!("{}.values", enum_fqn).into(),
        simple_name: "values".into(),
        range,
        kind: crate::SymbolKind::METHOD,
        modifiers: Modifiers {
            is_public: true,
            is_static: true,
            is_final: false,
            is_abstract: false,
            ..Default::default()
        },
        container: Some(enum_fqn.into()),
        return_type: Some(return_type.into()),
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        parameters: vec![],
        type_parameters: Vec::new(),
        is_deprecated: false,
        receiver_type: None, // Java doesn't have extension functions
        kotlin: None,
        throws: None,
    }
}

/// Generate `static T valueOf(String name)` method
fn generate_valueof_method(enum_fqn: &str, range: TextRange) -> Symbol {
    let enum_simple_name = enum_fqn.split('.').next_back().unwrap_or("");

    Symbol {
        fqn: format!("{}.valueOf", enum_fqn).into(),
        simple_name: "valueOf".into(),
        range,
        kind: crate::SymbolKind::METHOD,
        modifiers: Modifiers {
            is_public: true,
            is_static: true,
            is_final: false,
            is_abstract: false,
            ..Default::default()
        },
        container: Some(enum_fqn.into()),
        return_type: Some(enum_simple_name.into()),
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        parameters: vec!["java.lang.String name".into()],
        type_parameters: Vec::new(),
        is_deprecated: false,
        receiver_type: None, // Java doesn't have extension functions
        kotlin: None,
        throws: None,
    }
}

/// Generate `String name()` instance method
fn generate_name_method(enum_fqn: &str, range: TextRange) -> Symbol {
    Symbol {
        fqn: format!("{}.name", enum_fqn).into(),
        simple_name: "name".into(),
        range,
        kind: crate::SymbolKind::METHOD,
        modifiers: Modifiers {
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            ..Default::default()
        },
        container: Some(enum_fqn.into()),
        return_type: Some("String".into()),
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        parameters: vec![],
        type_parameters: Vec::new(),
        is_deprecated: false,
        receiver_type: None, // Java doesn't have extension functions
        kotlin: None,
        throws: None,
    }
}

/// Generate `int ordinal()` instance method
fn generate_ordinal_method(enum_fqn: &str, range: TextRange) -> Symbol {
    Symbol {
        fqn: format!("{}.ordinal", enum_fqn).into(),
        simple_name: "ordinal".into(),
        range,
        kind: crate::SymbolKind::METHOD,
        modifiers: Modifiers {
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            ..Default::default()
        },
        container: Some(enum_fqn.into()),
        return_type: Some("int".into()),
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        parameters: vec![],
        type_parameters: Vec::new(),
        is_deprecated: false,
        receiver_type: None, // Java doesn't have extension functions
        kotlin: None,
        throws: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_enum_methods() {
        let enum_info = EnumInfo {
            name: "Color".into(),
            constants: vec!["RED".to_string(), "GREEN".to_string(), "BLUE".to_string()],
            range: TextRange::new(0, 100),
        };

        let symbols =
            generate_enum_methods(&enum_info, "com.example.Color", TextRange::new(0, 100));

        assert_eq!(symbols.len(), 4);

        // values()
        assert_eq!(symbols[0].simple_name.as_ref(), "values");
        assert_eq!(symbols[0].return_type, Some("Color[]".into()));
        assert!(symbols[0].modifiers.is_static);

        // valueOf()
        assert_eq!(symbols[1].simple_name.as_ref(), "valueOf");
        assert_eq!(symbols[1].return_type, Some("Color".into()));
        assert_eq!(symbols[1].parameters, ["java.lang.String name".into()]);
        assert!(symbols[1].modifiers.is_static);

        // name()
        assert_eq!(symbols[2].simple_name.as_ref(), "name");
        assert_eq!(symbols[2].return_type, Some("String".into()));
        assert!(!symbols[2].modifiers.is_static);

        // ordinal()
        assert_eq!(symbols[3].simple_name.as_ref(), "ordinal");
        assert_eq!(symbols[3].return_type, Some("int".into()));
        assert!(!symbols[3].modifiers.is_static);
    }
}
