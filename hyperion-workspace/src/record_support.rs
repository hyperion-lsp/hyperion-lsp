// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Record synthetic method generation
//!
//! Java records (Java 14+) automatically generate:
//! - Accessor methods for each component (NO "get" prefix - just `name()`, `email()`)
//! - Constructor with all components
//! - `equals(Object)` method
//! - `hashCode()` method
//! - `toString()` method
//!
//! This module generates synthetic symbols for these methods.

use crate::{ImportMap, Modifiers, Symbol, TextRange};
use hyperion_parser::syntax::{SyntaxNode, TypeDecl};
use hyperion_types::{extract_and_resolve_type_without_lookup, ResolutionContext};

/// Information about a record component (parameter)
#[derive(Debug, Clone)]
pub struct RecordComponent {
    pub name: String,
    pub type_name: String,
    pub range: TextRange,
}

/// Information about a record extracted from AST
#[derive(Debug, Clone)]
pub struct RecordInfo {
    pub name: String,
    pub components: Vec<RecordComponent>,
    pub range: TextRange,
}

impl RecordInfo {
    /// Extract record info from a RECORD_DECL node
    pub fn from_node(
        node: &SyntaxNode,
        package: &Option<String>,
        imports: &ImportMap,
        record_fqn: &str,
    ) -> Option<Self> {
        // Cast to RecordDecl
        let type_decl = TypeDecl::cast(node.clone())?;
        let record_decl = match type_decl {
            TypeDecl::Record(r) => r,
            _ => return None,
        };

        // Get record name
        let name = record_decl.name()?.text().to_string();

        let context = ResolutionContext::with_wildcards(
            imports.explicit.clone(),
            imports.wildcards.clone(),
            package.clone(),
            Some(record_fqn.to_string()),
        );

        // Get record components from parameter list
        let mut components = Vec::new();
        if let Some(param_list) = record_decl.components() {
            for param in param_list.params() {
                if let Some(param_name_token) = param.name() {
                    let type_name = param
                        .ty()
                        .and_then(|ty| {
                            extract_and_resolve_type_without_lookup(ty.syntax(), &context)
                                .ok()
                                .map(|resolved| resolved.to_fqn_string())
                        })
                        .unwrap_or_else(|| "java.lang.Object".to_string());
                    let param_name = param_name_token.text().to_string();
                    let param_range = TextRange::new(
                        param_name_token.text_range().start().into(),
                        param_name_token.text_range().end().into(),
                    );

                    components.push(RecordComponent {
                        name: param_name,
                        type_name,
                        range: param_range,
                    });
                }
            }
        }

        Some(RecordInfo {
            name,
            components,
            range: TextRange::new(
                node.text_range().start().into(),
                node.text_range().end().into(),
            ),
        })
    }
}

/// Generate synthetic record methods
pub fn generate_record_methods(
    record_info: &RecordInfo,
    record_fqn: &str,
    record_range: TextRange,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    // Generate accessor methods for each component (NO "get" prefix)
    for component in &record_info.components {
        symbols.push(generate_accessor_method(
            component,
            record_fqn,
            component.range,
        ));
    }

    // Generate equals(Object) method
    symbols.push(generate_equals_method(record_fqn, record_range));

    // Generate hashCode() method
    symbols.push(generate_hashcode_method(record_fqn, record_range));

    // Generate toString() method
    symbols.push(generate_tostring_method(record_fqn, record_range));

    symbols
}

/// Generate accessor method for a record component (e.g., `name()` not `getName()`)
fn generate_accessor_method(
    component: &RecordComponent,
    record_fqn: &str,
    range: TextRange,
) -> Symbol {
    Symbol {
        fqn: format!("{}.{}()", record_fqn, component.name).into(),
        simple_name: component.name.clone().into(),
        range,
        kind: crate::SymbolKind::METHOD,
        modifiers: Modifiers {
            is_public: true,
            is_static: false,
            is_final: false,
            is_abstract: false,
            ..Default::default()
        },
        container: Some(record_fqn.to_string().into()),
        return_type: Some(component.type_name.clone().into()),
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

/// Generate `boolean equals(Object)` method
fn generate_equals_method(record_fqn: &str, range: TextRange) -> Symbol {
    Symbol {
        fqn: format!("{}.equals(java.lang.Object)", record_fqn).into(),
        simple_name: "equals".into(),
        range,
        kind: crate::SymbolKind::METHOD,
        modifiers: Modifiers {
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            ..Default::default()
        },
        container: Some(record_fqn.to_string().into()),
        return_type: Some("boolean".into()),
        parent_class: None,
        interfaces: vec![],
        documentation: None,
        parameters: vec!["java.lang.Object other".into()],
        type_parameters: Vec::new(),
        is_deprecated: false,
        receiver_type: None, // Java doesn't have extension functions
        kotlin: None,
        throws: None,
    }
}

/// Generate `int hashCode()` method
fn generate_hashcode_method(record_fqn: &str, range: TextRange) -> Symbol {
    Symbol {
        fqn: format!("{}.hashCode()", record_fqn).into(),
        simple_name: "hashCode".into(),
        range,
        kind: crate::SymbolKind::METHOD,
        modifiers: Modifiers {
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            ..Default::default()
        },
        container: Some(record_fqn.to_string().into()),
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

/// Generate `String toString()` method
fn generate_tostring_method(record_fqn: &str, range: TextRange) -> Symbol {
    Symbol {
        fqn: format!("{}.toString()", record_fqn).into(),
        simple_name: "toString".into(),
        range,
        kind: crate::SymbolKind::METHOD,
        modifiers: Modifiers {
            is_public: true,
            is_static: false,
            is_final: true,
            is_abstract: false,
            ..Default::default()
        },
        container: Some(record_fqn.to_string().into()),
        return_type: Some("java.lang.String".into()),
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
    fn test_generate_record_methods() {
        let record_info = RecordInfo {
            name: "User".into(),
            components: vec![
                RecordComponent {
                    name: "name".into(),
                    type_name: "java.lang.String".into(),
                    range: TextRange::new(10, 14), // "name"
                },
                RecordComponent {
                    name: "email".into(),
                    type_name: "java.lang.String".into(),
                    range: TextRange::new(20, 25), // "email"
                },
            ],
            range: TextRange::new(0, 100),
        };

        let symbols =
            generate_record_methods(&record_info, "com.example.User", TextRange::new(0, 100));

        // 2 accessors + equals + hashCode + toString = 5 methods
        assert_eq!(symbols.len(), 5);

        // name() accessor
        assert_eq!(symbols[0].simple_name.as_ref(), "name");
        assert_eq!(symbols[0].fqn.as_ref(), "com.example.User.name()");
        assert_eq!(symbols[0].return_type, Some("java.lang.String".into()));
        assert!(!symbols[0].modifiers.is_static);

        // email() accessor
        assert_eq!(symbols[1].simple_name.as_ref(), "email");
        assert_eq!(symbols[1].fqn.as_ref(), "com.example.User.email()");
        assert_eq!(symbols[1].return_type, Some("java.lang.String".into()));
        assert!(!symbols[1].modifiers.is_static);

        // equals()
        assert_eq!(symbols[2].simple_name.as_ref(), "equals");
        assert_eq!(
            symbols[2].fqn.as_ref(),
            "com.example.User.equals(java.lang.Object)"
        );
        assert_eq!(
            symbols[2].parameters.first().map(AsRef::as_ref),
            Some("java.lang.Object other")
        );
        assert_eq!(symbols[2].return_type, Some("boolean".into()));

        // hashCode()
        assert_eq!(symbols[3].simple_name.as_ref(), "hashCode");
        assert_eq!(symbols[3].fqn.as_ref(), "com.example.User.hashCode()");
        assert_eq!(symbols[3].return_type, Some("int".into()));

        // toString()
        assert_eq!(symbols[4].simple_name.as_ref(), "toString");
        assert_eq!(symbols[4].fqn.as_ref(), "com.example.User.toString()");
        assert_eq!(symbols[4].return_type, Some("java.lang.String".into()));
    }
}
