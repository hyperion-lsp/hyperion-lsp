// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::{expressions::infer_lambda_with_expected_type, TypeInferenceEngine};
use crate::db::FileId;
use crate::symbol::{ClassType, Type};
use hyperion_workspace::WorkspaceIndex;
use std::sync::Arc;
use tempfile::TempDir;

fn engine_for(source: &str) -> (TempDir, TypeInferenceEngine) {
    let temp = TempDir::new().unwrap();
    let workspace = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
    workspace
        .index_file_with_source(&temp.path().join("p/Functions.java"), source)
        .unwrap();
    let engine = TypeInferenceEngine::new(workspace, FileId::new(1));
    (temp, engine)
}

fn class_type(fqn: &str, type_arguments: Vec<Type>) -> Type {
    let (package, name) = fqn.rsplit_once('.').unwrap();
    Type::Class(ClassType::with_type_args(
        name.to_string(),
        Some(package.to_string()),
        type_arguments,
    ))
}

#[test]
fn lambda_expected_type_uses_inherited_sam_and_default_satisfaction() {
    let (_temp, engine) = engine_for(concat!(
        "package p; interface Parent { String call(); } ",
        "interface Child extends Parent {} ",
        "interface Satisfied extends Parent { default String call() { return null; } }"
    ));

    assert_eq!(
        infer_lambda_with_expected_type(&engine, &class_type("p.Child", vec![])),
        Type::Class(ClassType::string())
    );
    assert_eq!(
        infer_lambda_with_expected_type(&engine, &class_type("p.Satisfied", vec![])),
        Type::Unknown
    );
}

#[test]
fn lambda_expected_type_substitutes_direct_generic_sam() {
    let (_temp, engine) =
        engine_for("package p; interface Mapper<Input, Output> { Output map(Input input); }");
    let request = class_type("p.Request", vec![]);
    let response = class_type("p.Response", vec![]);
    let expected = class_type("p.Mapper", vec![request, response.clone()]);

    assert_eq!(
        infer_lambda_with_expected_type(&engine, &expected),
        response
    );
}

#[test]
fn lambda_expected_type_composes_inherited_generic_edge() {
    let (_temp, engine) = engine_for(concat!(
        "package p; interface Parent<T> { T call(); } ",
        "interface Child extends Parent<String> {}"
    ));

    assert!(matches!(
        infer_lambda_with_expected_type(&engine, &class_type("p.Child", vec![])),
        Type::Class(return_type) if return_type.name == "String"
    ));
}

#[test]
fn lambda_expected_type_rejects_method_generic_function_signature() {
    let (_temp, engine) =
        engine_for("package p; interface GenericMethod { <T> T identity(T value); }");
    assert_eq!(
        infer_lambda_with_expected_type(&engine, &class_type("p.GenericMethod", vec![])),
        Type::Unknown
    );
}
