// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type hierarchy through the real server: classes extend `java.lang.Object`.

use super::refactor_harness::Harness;
use tower_lsp::lsp_types::{
    PartialResultParams, TextDocumentIdentifier, TextDocumentPositionParams,
    TypeHierarchyPrepareParams, TypeHierarchySupertypesParams, WorkDoneProgressParams,
};
use tower_lsp::LanguageServer;

#[tokio::test(flavor = "multi_thread")]
async fn class_supertypes_include_object_and_interfaces_do_not() {
    let h = Harness::start(&[
        ("demo/Shape.java", "package demo;\n\npublic interface Shape {\n    double area();\n}\n"),
        (
            "demo/Circle.java",
            "package demo;\n\npublic class Circle implements Shape {\n    public double area() { return 1; }\n}\n",
        ),
    ])
    .await;
    h.open("demo/Circle.java").await;
    h.open("demo/Shape.java").await;

    let supertypes_of = |file: &'static str, needle: &'static str| {
        let h = &h;
        async move {
            let position = h.position_of(file, needle, 1);
            let items = h
                .backend
                .prepare_type_hierarchy(TypeHierarchyPrepareParams {
                    text_document_position_params: TextDocumentPositionParams::new(
                        TextDocumentIdentifier::new(h.uri(file)),
                        position,
                    ),
                    work_done_progress_params: WorkDoneProgressParams::default(),
                })
                .await;
            let items = items.ok().flatten()?;
            let supertypes = h
                .backend
                .supertypes(TypeHierarchySupertypesParams {
                    item: items.into_iter().next()?,
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                })
                .await
                .ok()
                .flatten()?;
            Some(
                supertypes
                    .into_iter()
                    .map(|item| item.name)
                    .collect::<Vec<_>>(),
            )
        }
    };

    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    loop {
        let circle = supertypes_of("demo/Circle.java", "Circle implements").await;
        if let Some(names) = circle {
            if names.contains(&"Object".to_string()) {
                assert!(names.contains(&"Shape".to_string()), "{names:?}");
                break;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Circle supertypes never included Object"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let shape = supertypes_of("demo/Shape.java", "Shape {").await;
    assert!(
        shape.is_none_or(|names| !names.contains(&"Object".to_string())),
        "interfaces do not list Object as a supertype"
    );
}
