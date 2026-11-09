// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_parser::{lexer::Lexer, parser::Parser, SyntaxKind, SyntaxNode};
use hyperion_semantics::{
    semantic_walker::compute_file_semantics, ResolverBuilder, SemanticErrorKind, TypeChecker,
};
use hyperion_workspace::WorkspaceIndex;

fn declaration_mismatches(argument: &str, child_import: &str) -> Vec<String> {
    let root = tempfile::tempdir().expect("workspace directory");
    let index = Arc::new(WorkspaceIndex::new(root.path().to_path_buf()));
    let files = [
        (
            "api/CatalogApi.java",
            "package api; public class CatalogApi { public interface Contract {} }",
        ),
        (
            "api/OtherApi.java",
            "package api; public class OtherApi { public interface Contract {} }",
        ),
        (
            "other/CatalogApi.java",
            "package other; public class CatalogApi { public interface Contract {} }",
        ),
        (
            "util/ServiceHandle.java",
            "package util; public class ServiceHandle<T> implements AutoCloseable { public void close() {} }",
        ),
        (
            "base/Parent.java",
            "package base; import api.CatalogApi; import util.ServiceHandle; public class Parent { protected ServiceHandle<CatalogApi.Contract> inheritedFactory() { return null; } }",
        ),
    ];
    for (relative, source) in files {
        index
            .index_file_with_source(&root.path().join(relative), source)
            .expect("index fixture");
    }
    let child = format!(
        "package app; import {child_import}; import api.OtherApi; import util.ServiceHandle; \
         class Child extends base.Parent {{ void test() {{ \
         try (ServiceHandle<{argument}> handle = inheritedFactory()) {{ }} \
         }} }}"
    );
    let child_path = root.path().join("app/Child.java");
    index
        .index_file_with_source(&child_path, &child)
        .expect("index subclass");
    let file_id = index.get_file_id(&child_path).expect("subclass file ID");
    let tokens = Lexer::new(&child).collect();
    let (green, errors) = Parser::parse(tokens);
    assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
    let syntax = SyntaxNode::new_root(green);
    assert!(
        syntax
            .descendants()
            .any(|node| node.kind() == SyntaxKind::LOCAL_VAR_DECL),
        "fixture must exercise variable declaration checking: {syntax:#?}"
    );
    let mut checker = TypeChecker::new();
    let resolver =
        ResolverBuilder::with_workspace(file_id, Arc::clone(&index)).build_from_syntax(&syntax);
    checker.set_file_semantics(Arc::new(compute_file_semantics(
        &syntax,
        file_id,
        resolver.symbol_table(),
        &index,
    )));
    checker
        .check_with_context(&syntax, &[], file_id, index)
        .into_iter()
        .filter_map(|error| match &error.kind {
            SemanticErrorKind::DeclarationTypeMismatch { .. } => Some(error.message()),
            _ => None,
        })
        .collect()
}

#[test]
fn inherited_return_generic_uses_declaring_import_context() {
    assert!(
        declaration_mismatches("CatalogApi.Contract", "api.CatalogApi").is_empty(),
        "short nested type must equal the declaration's qualified generic argument"
    );
    assert!(declaration_mismatches("api.CatalogApi.Contract", "api.CatalogApi").is_empty());
}

#[test]
fn different_nested_generic_argument_still_mismatches() {
    assert_eq!(
        declaration_mismatches("OtherApi.Contract", "api.CatalogApi").len(),
        1
    );
}

#[test]
fn return_type_uses_parent_import_even_when_child_import_shadows_it() {
    assert!(
        declaration_mismatches("api.CatalogApi.Contract", "other.CatalogApi").is_empty(),
        "the parent's imported CatalogApi must not resolve through the child's imports"
    );
    assert_eq!(
        declaration_mismatches("CatalogApi.Contract", "other.CatalogApi").len(),
        1
    );
}
