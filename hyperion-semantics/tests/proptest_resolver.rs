// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Property contracts for the production symbol resolver.
//!
//! These tests deliberately exercise Hyperion's parser, resolver builder, and
//! lexical lookup. They avoid restating scope or import algorithms in test code.

use hyperion_parser::{Lexer, Parser, SyntaxNode};
use hyperion_semantics::db::FileId;
use hyperion_semantics::resolver::{ResolverBuilder, SymbolResolver};
use hyperion_semantics::symbol::{
    ClassType, PrimitiveType, ScopeKind, SourceLocation, Symbol, SymbolKind, Type, Visibility,
};
use proptest::prelude::*;
use std::collections::HashSet;

fn java_identifier(prefix: char) -> impl Strategy<Value = String> {
    prop::string::string_regex(&format!("{prefix}[a-zA-Z0-9_]{{0,20}}"))
        .expect("valid Java identifier strategy")
}

fn variable_symbol(name: String, ty: Type, offset: u32) -> Symbol {
    Symbol::new(
        name,
        SymbolKind::LocalVariable {
            var_type: ty.clone(),
            is_final: false,
        },
        ty,
        Visibility::PackagePrivate,
        SourceLocation::new(FileId::new(1), offset, offset + 1),
    )
}

proptest! {
    /// Parsing and resolver construction must retain user declarations, even
    /// across identifier shapes that example-based fixtures rarely cover.
    #[test]
    fn resolver_builder_registers_generated_declarations(
        class_name in java_identifier('C'),
        field_name in java_identifier('f'),
        method_name in java_identifier('m'),
        parameter_name in java_identifier('p'),
    ) {
        let source = format!(
            "class {class_name} {{ int {field_name}; void {method_name}(int {parameter_name}) {{}} }}"
        );
        let tokens = Lexer::new(&source).collect();
        let (green_node, errors) = Parser::parse(tokens);
        prop_assert!(errors.is_empty(), "generated Java should parse: {errors:?}\n{source}");

        let root = SyntaxNode::new_root(green_node);
        let resolver = ResolverBuilder::new(FileId::new(1)).build_from_syntax(&root);
        let names: HashSet<String> = resolver
            .symbol_table()
            .all_symbols()
            .into_iter()
            .map(|symbol| symbol.name)
            .collect();

        for expected in [&class_name, &field_name, &method_name, &parameter_name] {
            prop_assert!(
                names.contains(expected),
                "resolver dropped {expected:?}; retained {names:?}\n{source}"
            );
        }
    }

    /// Lexical lookup must select the innermost declaration and invalidate its
    /// cached answer when that scope is left.
    #[test]
    fn lexical_resolution_honors_shadowing_and_scope_exit(name in java_identifier('v')) {
        let mut resolver = SymbolResolver::new();
        resolver.enter_scope(ScopeKind::Method);
        resolver.define_symbol(variable_symbol(
            name.clone(),
            Type::Primitive(PrimitiveType::Int),
            10,
        ));

        resolver.enter_scope(ScopeKind::Block);
        resolver.define_symbol(variable_symbol(
            name.clone(),
            Type::Primitive(PrimitiveType::Boolean),
            20,
        ));

        let inner = resolver.resolve(&name).expect("inner declaration should resolve");
        prop_assert_eq!(inner.ty, Type::Primitive(PrimitiveType::Boolean));

        prop_assert!(resolver.exit_scope());
        let outer = resolver.resolve(&name).expect("outer declaration should resolve after exit");
        prop_assert_eq!(outer.ty, Type::Primitive(PrimitiveType::Int));
    }

    /// Qualified and raw names are a public type-resolution boundary: generic
    /// arguments may affect display, but never lookup keys.
    #[test]
    fn raw_class_names_exclude_type_arguments(
        name in java_identifier('C'),
        package_parts in prop::collection::vec(java_identifier('p'), 1..4),
    ) {
        let package = package_parts.join(".");
        let mut class_type = ClassType::new(name.clone(), Some(package.clone()));
        class_type.type_arguments = vec![Type::Class(ClassType::string())];

        prop_assert_eq!(class_type.raw_name(), name.clone());
        prop_assert_eq!(class_type.raw_qualified_name(), format!("{package}.{name}"));
        prop_assert_eq!(class_type.qualified_name(), format!("{package}.{name}"));
    }
}
