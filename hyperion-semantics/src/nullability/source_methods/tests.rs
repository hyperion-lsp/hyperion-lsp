// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::{
    method_call_name_offset, owner_matches, simple_type_parts, type_match_score,
    visible_variable_binding, visible_variable_type, SourceMethodDeclaration, SourceMethodTable,
};
use crate::nullability::patterns::ValueKind;
use crate::nullability::{
    extract_method_info, find_enclosing_class_name, MethodSignatureCache, MethodSignatureSnapshot,
};
use hyperion_parser::{syntax::SyntaxKind, Lexer, Parser, SyntaxNode};
use std::collections::HashMap;
use std::sync::Arc;

#[test]
fn owner_matching_preserves_nested_source_owners() {
    assert!(owner_matches("pkg.Outer.Inner", "Outer.Inner"));
    assert!(!owner_matches("pkg.Other.Inner", "Outer.Inner"));
}

#[test]
fn descriptor_matching_ignores_only_package_qualification() {
    assert_eq!(simple_type_parts("java.lang.String[]"), ("String", "[]"));
    assert_eq!(simple_type_parts("java.lang.String..."), ("String", "..."));
}

#[test]
fn source_type_variable_returns_remain_unknown_without_receiver_substitution() {
    let source = "class Box<T> { T get() { return null; } }";
    let (green, errors) = Parser::parse(Lexer::new(source).collect());
    assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
    let root = SyntaxNode::new_root(green);
    let method = root
        .descendants()
        .find(|node| node.kind() == SyntaxKind::METHOD_DECL)
        .expect("get method");
    let info = extract_method_info(&method).expect("method info");
    let table = SourceMethodTable::new(
        [SourceMethodDeclaration::from_method(&method, "Box", &info)],
        None,
    );

    assert_eq!(
        table
            .binding_for_declaration("Box.get()")
            .expect("source binding")
            .2,
        ValueKind::Unknown
    );
}

#[test]
fn strict_reference_widening_outranks_loose_unboxing() {
    let object = type_match_score("Integer", "Object").expect("reference widening");
    let primitive = type_match_score("Integer", "int").expect("unboxing");
    assert!(
        object > primitive,
        "strict phase must win: {object:?} vs {primitive:?}"
    );
    assert_eq!(primitive, (1, 14));
}

#[test]
fn lexical_type_lookup_handles_foreach_and_catch_parameters() {
    let source = r#"
        class Test {
            Object select(String value) { return value; }
            Object select(Problem value) { return value; }
            void test(String[] values) {
                for (String value : values) { select(value); }
                try { work(); } catch (Problem problem) { select(problem); }
            }
        }
    "#;
    let (green, errors) = Parser::parse(Lexer::new(source).collect());
    assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
    let root = SyntaxNode::new_root(green);
    let arguments: Vec<_> = root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::NAME_EXPR)
        .filter(|node| {
            node.parent()
                .is_some_and(|parent| parent.kind() == SyntaxKind::ARG_LIST)
        })
        .filter(|node| matches!(node.text().to_string().as_str(), "value" | "problem"))
        .collect();
    assert_eq!(arguments.len(), 2);
    assert_eq!(
        visible_variable_type(&arguments[0], "value", &HashMap::new()).as_deref(),
        Some("String")
    );
    assert_eq!(
        visible_variable_type(&arguments[1], "problem", &HashMap::new()).as_deref(),
        Some("Problem")
    );

    let methods: Vec<_> = root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::METHOD_DECL)
        .filter_map(|method| {
            let info = extract_method_info(&method)?;
            Some(SourceMethodDeclaration::from_method(&method, "Test", &info))
        })
        .collect();
    let table = SourceMethodTable::new(methods, None);
    let test_method = root
        .descendants()
        .find(|node| {
            node.kind() == SyntaxKind::METHOD_DECL
                && extract_method_info(node).is_some_and(|info| info.name == "test")
        })
        .expect("test method");
    let call_owners = test_method
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
        .filter_map(|call| {
            method_call_name_offset(&call).map(|offset| (offset, "Test".to_string()))
        })
        .collect();
    let resolved = table.resolve_calls(
        hyperion_workspace::FileId::new(1),
        &test_method,
        &HashMap::new(),
        &call_owners,
        &MethodSignatureSnapshot::empty(),
    );
    let selected: Vec<_> = resolved.values().map(AsRef::as_ref).collect();
    assert!(selected.contains(&"Test.select(String)"), "{resolved:?}");
    assert!(selected.contains(&"Test.select(Problem)"), "{resolved:?}");
}

#[test]
fn implicit_uppercase_lambda_parameter_is_a_value_without_a_recovered_type() {
    let source = r#"
        class Test {
            void test() { consume(VALUE -> VALUE.call()); }
        }
    "#;
    let (green, errors) = Parser::parse(Lexer::new(source).collect());
    assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
    let root = SyntaxNode::new_root(green);
    let value_use = root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::NAME_EXPR)
        .find(|node| node.text() == "VALUE")
        .expect("lambda parameter use");

    assert_eq!(
        visible_variable_type(&value_use, "VALUE", &HashMap::new()),
        None
    );
    assert!(visible_variable_binding(&value_use, "VALUE"));
}

#[test]
fn indexed_inherited_declaration_selects_the_declaring_source_owner() {
    let source = r#"
        package app;
        class Base { static void accept(int value) {} }
        class Child extends Base {}
        class Client {
            void run(Child child, Integer value) { child.accept(value); }
        }
    "#;
    let (green, errors) = Parser::parse(Lexer::new(source).collect());
    assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
    let root = SyntaxNode::new_root(green);
    let declarations = root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::METHOD_DECL)
        .filter_map(|method| {
            let info = extract_method_info(&method)?;
            let owner = find_enclosing_class_name(&method)?;
            Some(SourceMethodDeclaration::from_method(&method, &owner, &info))
        });
    let table = SourceMethodTable::new(declarations, Some("app"));
    let client_method = root
        .descendants()
        .find(|node| {
            node.kind() == SyntaxKind::METHOD_DECL
                && extract_method_info(node).is_some_and(|info| info.name == "run")
        })
        .expect("client method");
    let call = client_method
        .descendants()
        .find(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
        .expect("method call");
    let offset = method_call_name_offset(&call).expect("call offset");
    let cache = MethodSignatureCache::new();
    assert!(cache.update_indexed_call_bindings(
        hyperion_workspace::FileId::new(1),
        11,
        vec![(offset, Arc::from("app.Base.accept(int)"))],
    ));

    let resolved = table.resolve_calls(
        hyperion_workspace::FileId::new(1),
        &client_method,
        &HashMap::new(),
        &HashMap::from([(offset, "Child".to_string())]),
        &cache.snapshot(),
    );

    assert_eq!(
        resolved.get(&offset).map(AsRef::as_ref),
        Some("Base.accept(int)")
    );
}

#[test]
fn indexed_external_declaration_never_falls_back_to_a_similar_local_method() {
    let source = r#"
        package app;
        class Service { static void accept(int value) {} }
        class Client { void run() { Service.accept(1); } }
    "#;
    let (green, errors) = Parser::parse(Lexer::new(source).collect());
    assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
    let root = SyntaxNode::new_root(green);
    let declarations = root
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::METHOD_DECL)
        .filter_map(|method| {
            let info = extract_method_info(&method)?;
            let owner = find_enclosing_class_name(&method)?;
            Some(SourceMethodDeclaration::from_method(&method, &owner, &info))
        });
    let table = SourceMethodTable::new(declarations, Some("app"));
    let client_method = root
        .descendants()
        .find(|node| {
            node.kind() == SyntaxKind::METHOD_DECL
                && extract_method_info(node).is_some_and(|info| info.name == "run")
        })
        .expect("client method");
    let call = client_method
        .descendants()
        .find(|node| node.kind() == SyntaxKind::METHOD_CALL_EXPR)
        .expect("method call");
    let offset = method_call_name_offset(&call).expect("call offset");
    let cache = MethodSignatureCache::new();
    assert!(cache.update_indexed_call_bindings(
        hyperion_workspace::FileId::new(1),
        11,
        vec![(offset, Arc::from("dependency.Service.accept(int)"))],
    ));

    let resolved = table.resolve_calls(
        hyperion_workspace::FileId::new(1),
        &client_method,
        &HashMap::new(),
        &HashMap::from([(offset, "Service".to_string())]),
        &cache.snapshot(),
    );

    assert!(!resolved.contains_key(&offset), "{resolved:?}");
}
