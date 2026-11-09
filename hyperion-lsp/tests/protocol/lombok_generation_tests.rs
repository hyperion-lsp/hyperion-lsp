// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Generate constructor / getters / setters / equals / toString must not
//! duplicate what Lombok or the class already provides.

use super::refactor_harness::{apply_workspace_edit, find_action, title_of, Harness};
use tower_lsp::lsp_types::CodeActionOrCommand;

async fn actions_for(source: &str, class_marker: &str) -> (Harness, Vec<CodeActionOrCommand>) {
    let h = Harness::start(&[("demo/Model.java", source)]).await;
    h.open("demo/Model.java").await;
    let position = h.position_of("demo/Model.java", class_marker, 2);
    // Generation only needs the open document's parse tree, which didOpen has
    // published by the time it returns.
    let actions = h
        .code_actions_until("demo/Model.java", position, |_| true)
        .await;
    (h, actions)
}

fn generate_titles(actions: &[CodeActionOrCommand]) -> Vec<String> {
    actions
        .iter()
        .map(title_of)
        .filter(|title| title.starts_with("Generate"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn plain_class_offers_everything_and_skips_static_fields_and_existing_methods() {
    let (h, actions) = actions_for(
        "package demo;\n\npublic class Model {\n    static int COUNT;\n    private final String id;\n    private String name;\n\n    public String getName() {\n        return name;\n    }\n}\n",
        "class Model",
    )
    .await;
    let titles = generate_titles(&actions);
    assert!(
        titles.contains(&"Generate constructor with 2 fields".to_string()),
        "{titles:?}"
    );
    assert!(
        titles.contains(&"Generate toString()".to_string()),
        "{titles:?}"
    );
    assert!(
        titles.contains(&"Generate equals() and hashCode()".to_string()),
        "{titles:?}"
    );
    // getName exists and id is final: only getId and setName are missing.
    let accessors = find_action(&actions, "Generate getters and setters for 2 fields")
        .unwrap_or_else(|| panic!("accessors action missing: {titles:?}"));
    apply_workspace_edit(accessors.edit.as_ref().unwrap());
    let text = h.read("demo/Model.java");
    assert!(text.contains("public String getId()"), "{text}");
    assert!(text.contains("public void setName(String name)"), "{text}");
    assert_eq!(
        text.matches("getName()").count(),
        1,
        "existing getter duplicated:\n{text}"
    );
    assert!(!text.contains("setId"), "final field got a setter:\n{text}");
    assert!(!text.contains("COUNT("), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn lombok_data_generates_everything_but_the_all_args_constructor() {
    let (_h, actions) = actions_for(
        "package demo;\n\nimport lombok.Data;\n\n@Data\npublic class Model {\n    private String id;\n    private String name;\n}\n",
        "class Model",
    )
    .await;
    let titles = generate_titles(&actions);
    assert!(
        titles
            .iter()
            .all(|t| !t.contains("getters") && !t.contains("toString") && !t.contains("equals")),
        "@Data already generates these: {titles:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lombok_getter_and_setter_annotations_are_respected_per_class_and_field() {
    let (h, actions) = actions_for(
        "package demo;\n\nimport lombok.Getter;\nimport lombok.Setter;\n\n@Getter\npublic class Model {\n    private String id;\n    @Setter\n    private String name;\n    private int size;\n}\n",
        "class Model",
    )
    .await;
    let titles = generate_titles(&actions);
    // Getters come from @Getter, setName from @Setter: only setId and setSize are missing.
    let accessors = find_action(&actions, "Generate getters and setters for 2 fields")
        .unwrap_or_else(|| panic!("accessors action missing: {titles:?}"));
    apply_workspace_edit(accessors.edit.as_ref().unwrap());
    let text = h.read("demo/Model.java");
    assert!(
        text.contains("setId(") && text.contains("setSize("),
        "{text}"
    );
    assert!(
        !text.contains("getId(") && !text.contains("setName("),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lombok_value_and_all_args_constructor_suppress_the_constructor() {
    let (_h, actions) = actions_for(
        "package demo;\n\n@Value\npublic class Model {\n    String id;\n    String name;\n}\n",
        "class Model",
    )
    .await;
    let titles = generate_titles(&actions);
    assert!(
        titles
            .iter()
            .all(|t| !t.contains("constructor") && !t.contains("getters")),
        "@Value already provides these: {titles:?}"
    );
    let (_h2, actions) = actions_for(
        "package demo;\n\n@AllArgsConstructor\npublic class Model {\n    private String id;\n}\n",
        "class Model",
    )
    .await;
    let titles = generate_titles(&actions);
    assert!(
        titles.iter().all(|t| !t.contains("constructor")),
        "{titles:?}"
    );
    assert!(
        titles.iter().any(|t| t.contains("getters and setters")),
        "{titles:?}"
    );
}
