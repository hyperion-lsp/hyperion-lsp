// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type resolution logic for method return types and field types
//!
//! This module handles resolving types from the workspace index and parsing
//! type strings into the internal Type representation.

use super::SymbolResolver;
use crate::call_resolution::resolve_call;
use crate::symbol::{ClassType, ScopeId, Type};
use crate::type_parser;
use hyperion_workspace::{ResolvedDefinition, WorkspaceIndex};
use tracing::debug;

/// Resolve one inherited field and its receiver-relative type without
/// separating field election from generic projection.
#[must_use]
pub fn resolve_effective_field(
    workspace_index: &WorkspaceIndex,
    receiver_type: &Type,
    field_name: &str,
) -> Option<(ResolvedDefinition, Type)> {
    let Type::Class(class_type) = receiver_type else {
        return None;
    };
    let receiver_fqn = class_type.qualified_name();
    resolve_effective_field_for_owner(workspace_index, &receiver_fqn, receiver_type, field_name)
}

/// Variant for callers that resolve a simple/imported receiver name in their
/// own source context before entering the workspace hierarchy.
#[must_use]
pub fn resolve_effective_field_for_owner(
    workspace_index: &WorkspaceIndex,
    receiver_fqn: &str,
    receiver_type: &Type,
    field_name: &str,
) -> Option<(ResolvedDefinition, Type)> {
    let Type::Class(class_type) = receiver_type else {
        return None;
    };
    let receiver_arguments = class_type
        .type_arguments
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let projected = workspace_index.find_projected_effective_field_by_name(
        receiver_fqn,
        &receiver_arguments,
        field_name,
    )?;
    // This helper intentionally models Java field expressions. Kotlin/Groovy
    // PROPERTY and CONSTANT lookup has language-specific access semantics.
    if projected.definition.symbol.kind != lsp_types::SymbolKind::FIELD {
        return None;
    }
    let field_type = type_parser::parse_type_string(projected.projected_type.as_deref()?)?;
    Some((projected.definition, field_type))
}

impl SymbolResolver {
    /// Get the return type of a method on a given type
    ///
    /// For a method call like `obj.method()`, this looks up the method
    /// on the receiver type and returns its return type.
    ///
    /// Requires workspace_index to be set via `set_workspace_index()`.
    pub fn get_method_return_type(&self, receiver_type: &Type, method_name: &str) -> Option<Type> {
        let workspace_index = self.workspace_index.as_ref()?;

        // Extract the FQN from the receiver type
        let receiver_fqn = if let Type::Class(class_type) = receiver_type {
            class_type.qualified_name()
        } else {
            debug!(
                "get_method_return_type: receiver is not a class type: {:?}",
                receiver_type
            );
            return None;
        };

        debug!(
            "get_method_return_type: looking up {}.{}",
            receiver_fqn, method_name
        );

        // This API represents a zero-argument invocation. Delegate applicability and
        // inheritance to the canonical call resolver rather than returning whichever
        // same-name declaration happens to be indexed first.
        let resolved = resolve_call(
            workspace_index,
            &receiver_fqn,
            Some(receiver_type),
            method_name,
            &[],
        );

        let Some(resolved) = resolved else {
            debug!(
                "get_method_return_type: no applicable zero-argument method '{}' found on '{}'",
                method_name, receiver_fqn
            );
            return None;
        };

        debug!(
            "get_method_return_type: resolved '{}' with return type '{}'",
            resolved.declaration.fqn, resolved.return_type
        );
        Some(resolved.return_type)
    }

    /// Resolve the type of a field on a given type
    ///
    /// For a field access like `obj.field`, this looks up the field
    /// on the receiver type and returns its type.
    ///
    /// Requires workspace_index to be set via `set_workspace_index()`.
    pub fn resolve_field_type(&self, receiver_type: &Type, field_name: &str) -> Option<Type> {
        let workspace_index = self.workspace_index.as_ref()?;

        // Extract the FQN from the receiver type
        let receiver_fqn = if let Type::Class(class_type) = receiver_type {
            class_type.qualified_name()
        } else {
            debug!(
                "resolve_field_type: receiver is not a class type: {:?}",
                receiver_type
            );
            return None;
        };

        debug!(
            "resolve_field_type: looking up {}.{}",
            receiver_fqn, field_name
        );

        let Some((definition, field_type)) =
            resolve_effective_field(workspace_index, receiver_type, field_name)
        else {
            debug!(
                "resolve_field_type: no unambiguous field '{}' found on '{}'",
                field_name, receiver_fqn
            );
            return None;
        };
        debug!(
            "resolve_field_type: found field '{}' with projected type '{}'",
            definition.target.fqn, field_type
        );
        Some(field_type)
    }

    /// Get the enclosing class type
    ///
    /// Returns the ClassType for the class that contains the current scope.
    /// Used for resolving `this` expressions.
    pub fn get_enclosing_class(&self) -> Option<ClassType> {
        let class_scope_id = self.enclosing_class_scope()?;
        self.get_class_type_from_scope(class_scope_id)
    }

    /// Get the class enclosing an exact source position without mutating the
    /// resolver's construction-time scope stack.
    pub fn get_enclosing_class_at_offset(&self, offset: u32) -> Option<ClassType> {
        let scope_id = self.find_scope_at_offset(offset)?;
        let class_scope_id = self.symbol_table.find_enclosing_class(scope_id)?;
        self.get_class_type_from_scope(class_scope_id)
    }

    fn get_class_type_from_scope(&self, class_scope_id: ScopeId) -> Option<ClassType> {
        let scope = self.symbol_table.get_scope(class_scope_id)?;
        let enclosing_range = scope.range;

        // Kotlin stores the declaration symbol in its class scope, while the
        // Java builder stores it in the enclosing scope so sibling declarations
        // can resolve the name. Inspect both, then use the earliest class-name
        // range inside this class node. This also avoids mistaking a Java
        // nested-class declaration stored in the outer class scope for the
        // outer class itself.
        let parent_symbols = scope
            .parent
            .and_then(|parent| self.symbol_table.get_scope(parent))
            .into_iter()
            .flat_map(|parent| parent.symbols.values());
        let symbol = scope
            .symbols
            .values()
            .chain(parent_symbols)
            .filter_map(|symbol_id| self.symbol_table.get_symbol(*symbol_id))
            .filter(|symbol| matches!(symbol.kind, crate::symbol::SymbolKind::Class { .. }))
            .filter(|symbol| {
                enclosing_range.is_none_or(|(start, end)| {
                    symbol.location.range.start >= start && symbol.location.range.end <= end
                })
            })
            .min_by_key(|symbol| (symbol.location.range.start, symbol.location.range.end))?;
        let Type::Class(class_type) = &symbol.type_info else {
            debug!(
                "get_enclosing_class: class symbol '{}' has non-class type {:?}",
                symbol.name, symbol.type_info
            );
            return None;
        };
        debug!(
            "get_enclosing_class: found class '{}' as '{}'",
            symbol.name,
            class_type.qualified_name()
        );
        Some(class_type.clone())
    }

    /// Get the super class of the enclosing class
    ///
    /// Returns the ClassType for the superclass of the current class.
    /// Used for resolving `super` expressions.
    pub fn get_super_class(&self) -> Option<ClassType> {
        let enclosing_class = self.get_enclosing_class()?;
        self.get_super_class_for(&enclosing_class)
    }

    /// Get the superclass of the class enclosing an exact source position.
    pub fn get_super_class_at_offset(&self, offset: u32) -> Option<ClassType> {
        let enclosing_class = self.get_enclosing_class_at_offset(offset)?;
        self.get_super_class_for(&enclosing_class)
    }

    fn get_super_class_for(&self, enclosing_class: &ClassType) -> Option<ClassType> {
        let workspace_index = self.workspace_index.as_ref()?;

        // Get the class FQN - try with package if available, otherwise just the simple name
        let class_fqn = enclosing_class.qualified_name();
        debug!("get_super_class: looking up parent of '{}'", class_fqn);

        // Look up the parent class from the workspace index
        let parent_fqn = workspace_index.get_parent_class(&class_fqn)?;
        debug!("get_super_class: found parent class '{}'", parent_fqn);

        // Parse the parent FQN into a ClassType
        if let Some((package, name)) = parent_fqn.rsplit_once('.') {
            Some(ClassType::new(name.to_string(), Some(package.to_string())))
        } else {
            // No package, just the simple name
            Some(ClassType::new(parent_fqn, None))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use hyperion_parser::{syntax::SyntaxNode, Lexer, Parser};
    use hyperion_workspace::WorkspaceIndex;

    use crate::resolver::ResolverBuilder;

    use super::*;

    #[test]
    fn enclosing_nested_class_keeps_its_qualified_identity_for_super_lookup() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        workspace
            .index_file_with_source(
                &temp.path().join("p/Base.java"),
                "package p; public class Base {}",
            )
            .expect("base should index");
        let source = concat!(
            "package p; ",
            "class Outer { class Nested extends Base { void marker() {} } }"
        );
        workspace
            .index_file_with_source(&temp.path().join("p/Outer.java"), source)
            .expect("nested class should index");

        let tokens = Lexer::new(source).collect::<Vec<_>>();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture must parse: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let mut resolver = ResolverBuilder::new(crate::db::FileId::new(1)).build_from_syntax(&root);
        resolver.set_workspace_index(Arc::clone(&workspace));
        let marker_offset = u32::try_from(source.find("marker").unwrap()).unwrap();
        let marker_scope = resolver
            .find_scope_at_offset(marker_offset)
            .expect("marker scope");
        resolver.scope_stack.push(marker_scope);

        assert_eq!(
            resolver
                .get_enclosing_class()
                .expect("enclosing class")
                .qualified_name(),
            "p.Outer.Nested"
        );
        assert_eq!(
            resolver
                .get_super_class()
                .expect("nested superclass")
                .qualified_name(),
            "p.Base"
        );
    }

    #[test]
    fn method_return_type_requires_an_applicable_zero_argument_declaration() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        workspace
            .index_file_with_source(
                &temp.path().join("com/example/Service.java"),
                r#"
                    package com.example;
                    public class Service {
                        String status(int code) { return ""; }
                        Object ready() { return null; }
                    }
                "#,
            )
            .expect("service should index");

        let mut resolver = SymbolResolver::new();
        resolver.set_workspace_index(workspace);
        let receiver = Type::Class(ClassType::new(
            "Service".to_string(),
            Some("com.example".to_string()),
        ));

        assert_eq!(
            resolver.get_method_return_type(&receiver, "ready"),
            Some(Type::Class(ClassType::object()))
        );
        assert_eq!(resolver.get_method_return_type(&receiver, "status"), None);
    }

    // A generic field's declared type variable must be substituted
    // with the receiver's type argument: `Box<String> b; b.value` where
    // `value: T` resolves to String, not to a class literally named `T`.
    #[test]
    fn field_type_substitutes_receiver_type_arguments() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        workspace
            .index_file_with_source(
                &temp.path().join("com/example/Box.java"),
                r#"
                    package com.example;
                    public class Box<T> {
                        public T value;
                    }
                "#,
            )
            .expect("box should index");

        let mut resolver = SymbolResolver::new();
        resolver.set_workspace_index(workspace);
        let receiver = crate::type_parser::parse_type_string("com.example.Box<java.lang.String>")
            .expect("receiver type should parse");

        match resolver.resolve_field_type(&receiver, "value") {
            Some(Type::Class(ct)) => assert_eq!(
                ct.name, "String",
                "field type T must be substituted with the receiver's String argument"
            ),
            other => panic!("Expected Class type for substituted field, got {other:?}"),
        }
    }

    // Control: a non-generic field keeps its declared type.
    #[test]
    fn field_type_without_type_variables_is_preserved() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        workspace
            .index_file_with_source(
                &temp.path().join("com/example/Holder.java"),
                r#"
                    package com.example;
                    public class Holder<T> {
                        public String label;
                    }
                "#,
            )
            .expect("holder should index");

        let mut resolver = SymbolResolver::new();
        resolver.set_workspace_index(workspace);
        let receiver =
            crate::type_parser::parse_type_string("com.example.Holder<java.lang.Integer>")
                .expect("receiver type should parse");

        match resolver.resolve_field_type(&receiver, "label") {
            Some(Type::Class(ct)) => assert_eq!(ct.name, "String"),
            other => panic!("Expected String field type, got {other:?}"),
        }
    }

    #[test]
    fn inherited_field_type_composes_fixed_multilevel_arguments() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        workspace
            .index_file_with_source(
                &temp.path().join("p/Types.java"),
                concat!(
                    "package p; class Value {} class Base<T> { public T value; } ",
                    "class Middle<U> extends Base<java.util.List<U>> {} ",
                    "class Child extends Middle<Value> {}"
                ),
            )
            .expect("hierarchy should index");

        let mut resolver = SymbolResolver::new();
        resolver.set_workspace_index(workspace);
        let receiver = Type::Class(ClassType::new("Child".to_string(), Some("p".to_string())));

        let Some(Type::Class(list)) = resolver.resolve_field_type(&receiver, "value") else {
            panic!("expected projected List<String> field type")
        };
        assert_eq!(list.name, "List");
        assert_eq!(
            list.type_arguments,
            [crate::type_parser::parse_type_string("p.Value").unwrap()]
        );
    }
}
