// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Safe delete through the real server: action -> command -> applyEdit.

use super::refactor_harness::{title_of, Harness};
use tower_lsp::lsp_types::{CodeActionOrCommand, ExecuteCommandParams, WorkDoneProgressParams};
use tower_lsp::LanguageServer;

const UNUSED: &str = "package demo;\n\npublic class Unused {\n    public int helper() {\n        return 1;\n    }\n}\n";
const MAIN: &str = "package demo;\n\npublic class Main {\n    public static void main(String[] args) {\n        System.out.println(\"hi\");\n    }\n}\n";

#[tokio::test(flavor = "multi_thread")]
async fn safe_delete_of_an_unused_class_applies_through_the_client() {
    let h = Harness::start(&[("demo/Unused.java", UNUSED), ("demo/Main.java", MAIN)]).await;
    h.open("demo/Unused.java").await;
    let position = h.position_of("demo/Unused.java", "Unused {", 1);
    let actions = h
        .code_actions_until("demo/Unused.java", position, |actions| {
            actions
                .iter()
                .any(|a| title_of(a).starts_with("Safe Delete"))
        })
        .await;
    let action = actions
        .iter()
        .find_map(|a| match a {
            CodeActionOrCommand::CodeAction(action) if action.title.starts_with("Safe Delete") => {
                Some(action.clone())
            }
            _ => None,
        })
        .unwrap();
    let command = action.command.expect("safe delete is a command");
    let result = h
        .backend
        .execute_command(ExecuteCommandParams {
            command: command.command,
            arguments: command.arguments.unwrap_or_default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .expect("executeCommand");
    assert_eq!(
        result,
        Some(serde_json::json!({ "applied": true })),
        "safe delete must apply; server messages: {:?}",
        h.server_messages
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .collect::<Vec<_>>()
    );
    let apply = h
        .server_messages
        .lock()
        .unwrap()
        .iter()
        .find(|(method, _)| method == "workspace/applyEdit")
        .map(|(_, params)| params.clone())
        .expect("workspace/applyEdit was sent");
    let edit: tower_lsp::lsp_types::ApplyWorkspaceEditParams =
        serde_json::from_value(apply).unwrap();
    super::refactor_harness::apply_workspace_edit(&edit.edit);
    assert!(
        !h.exists("demo/Unused.java"),
        "deleting the only top-level type must delete its file, not leave an empty one"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn code_action_resolve_is_the_identity() {
    let h = Harness::start(&[("demo/Main.java", MAIN)]).await;
    let action = tower_lsp::lsp_types::CodeAction {
        title: "Safe Delete 'Main'".to_string(),
        data: Some(serde_json::json!({ "hyperionSafeDelete": true })),
        ..Default::default()
    };
    let resolved = h
        .backend
        .code_action_resolve(action.clone())
        .await
        .expect("codeAction/resolve must not be method-not-found");
    assert_eq!(resolved, action);
}
