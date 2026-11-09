// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::lambda_inference::{
    extract_lambda_type_from_param, infer_lambda_parameter_type_with_workspace,
};
use hyperion_parser::{lexer::Lexer, parser::Parser, syntax::SyntaxNode};
use hyperion_semantics::symbol::{ClassType, Type};
use hyperion_workspace::index::WorkspaceIndex;
use std::sync::Arc;
use tempfile::TempDir;

fn parse_java(source: &str) -> SyntaxNode {
    let tokens = Lexer::new(source).collect();
    let (green, errors) = Parser::parse(tokens);
    assert!(errors.is_empty(), "parse errors: {errors:?}");
    SyntaxNode::new_root(green)
}

#[test]
fn indexed_custom_functional_interface_infers_lambda_parameter() {
    let temp = TempDir::new().unwrap();
    let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
    workspace
        .index_file_with_source(
            &temp.path().join("p/Action.java"),
            "package p; interface Action<T> { void accept(T value); }",
        )
        .unwrap();

    let result = extract_lambda_type_from_param(Some(&workspace), "p.Action<p.User> action", 0)
        .expect("indexed custom SAM parameter");
    assert_eq!(result.name, "User");
    assert_eq!(result.package.as_deref(), Some("p"));
}

#[test]
fn indexed_inherited_generic_functional_interface_projects_parameter() {
    let temp = TempDir::new().unwrap();
    let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
    workspace
        .index_file_with_source(
            &temp.path().join("p/Action.java"),
            concat!(
                "package p; interface Action<T> { void accept(T value); } ",
                "interface StringAction extends Action<String> {}"
            ),
        )
        .unwrap();

    assert_eq!(
        extract_lambda_type_from_param(Some(&workspace), "p.StringAction", 0),
        Some(ClassType::new(
            "String".to_string(),
            Some("java.lang".to_string())
        ))
    );
}

#[test]
fn indexed_invalid_standard_interface_does_not_use_the_standard_table() {
    let temp = TempDir::new().unwrap();
    let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
    workspace
        .index_file_with_source(
            &temp.path().join("java/util/function/Function.java"),
            concat!(
                "package java.util.function; interface Function<T, R> { ",
                "R apply(T value); R second(T value); }"
            ),
        )
        .unwrap();

    assert!(extract_lambda_type_from_param(
        Some(&workspace),
        "java.util.function.Function<String, Integer>",
        0
    )
    .is_none());
}

#[test]
fn indexed_invalid_standard_interface_does_not_use_the_platform_table() {
    let temp = TempDir::new().unwrap();
    let workspace = WorkspaceIndex::new(temp.path().to_path_buf());
    workspace
        .index_file_with_source(
            &temp.path().join("java/util/function/Function.java"),
            concat!(
                "package java.util.function; interface Function<T, R> { ",
                "R apply(T value); R second(T value); }"
            ),
        )
        .unwrap();

    assert!(
        extract_lambda_type_from_param(Some(&workspace), "Function<String, Integer>", 0).is_none()
    );
}

#[test]
fn custom_variable_declaration_uses_indexed_functional_interface() {
    let temp = TempDir::new().unwrap();
    let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    let source = concat!(
        "package p; interface Action<T> { void accept(T value); } ",
        "class C { void test() { p.Action<p.User> action = user -> user.toString(); } }"
    );
    workspace
        .index_file_with_source(&temp.path().join("p/Types.java"), source)
        .unwrap();
    let root = parse_java(source);
    let offset = source.find("user ->").expect("lambda parameter") as u32;
    let result = infer_lambda_parameter_type_with_workspace(&root, "user", offset, Some(workspace));

    assert_eq!(
        result,
        Some(Type::Class(ClassType::new(
            "User".to_string(),
            Some("p".to_string())
        )))
    );
}
