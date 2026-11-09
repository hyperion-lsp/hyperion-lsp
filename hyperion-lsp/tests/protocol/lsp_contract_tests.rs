// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Small, high-value contracts at the real `LanguageServer` boundary.
//!
//! These are deliberately workflows rather than feature matrices. Provider
//! unit tests cover syntax variants; this suite catches broken wiring between
//! document synchronization, semantic state, completion, navigation, and
//! refactoring.

use std::path::Path;
use std::sync::Arc;

use hyperion_lsp::server::Backend;
use tempfile::TempDir;
use tower_lsp::lsp_types::{
    CodeActionClientCapabilities, CodeActionLiteralSupport, CompletionItem, CompletionParams,
    CompletionResponse, DidChangeTextDocumentParams, DidOpenTextDocumentParams, DocumentChanges,
    GotoDefinitionParams, GotoDefinitionResponse, InitializeParams, InitializedParams, OneOf,
    PartialResultParams, Position, Range, RenameParams, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams,
};
use tower_lsp::{LanguageServer, LspService};

const USER_SOURCE: &str = concat!(
    "package model;\n",
    "public final class User {\n",
    "    public String displayName() { return \"Ada\"; }\n",
    "}\n",
);

const ACCOUNT_SOURCE: &str = concat!(
    "package model;\n",
    "public final class Account {\n",
    "    public String accountNumber() { return \"A-1\"; }\n",
    "}\n",
);

const MAIN_SOURCE: &str = concat!(
    "package app;\n",
    "\n",
    "import model.Account;\n",
    "import model.User;\n",
    "\n",
    "final class Main {\n",
    "    void render(/* 😀 */ User value) {\n",
    "        value.accountNumber();\n",
    "        value.displayName();\n",
    "    }\n",
    "}\n",
);

fn test_client() -> tower_lsp::Client {
    struct Dummy;

    #[tower_lsp::async_trait]
    impl LanguageServer for Dummy {
        async fn initialize(
            &self,
            _: tower_lsp::lsp_types::InitializeParams,
        ) -> tower_lsp::jsonrpc::Result<tower_lsp::lsp_types::InitializeResult> {
            Ok(tower_lsp::lsp_types::InitializeResult::default())
        }

        async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
            Ok(())
        }
    }

    let holder = Arc::new(std::sync::Mutex::new(None));
    let captured = Arc::clone(&holder);
    let (_service, _socket) = LspService::new(move |client| {
        *captured.lock().expect("client holder") = Some(client.clone());
        Dummy
    });

    let client = holder
        .lock()
        .expect("client holder")
        .take()
        .expect("client captured");
    client
}

#[tokio::test]
async fn code_actions_are_advertised_only_to_clients_supporting_literals() {
    let legacy = Backend::new(test_client());
    let capabilities = legacy
        .initialize(InitializeParams::default())
        .await
        .expect("legacy initialization")
        .capabilities;
    assert!(capabilities.code_action_provider.is_none());

    let modern = Backend::new(test_client());
    let mut params = InitializeParams::default();
    params.capabilities.text_document =
        Some(tower_lsp::lsp_types::TextDocumentClientCapabilities {
            code_action: Some(CodeActionClientCapabilities {
                code_action_literal_support: Some(CodeActionLiteralSupport::default()),
                ..Default::default()
            }),
            ..Default::default()
        });
    let capabilities = modern
        .initialize(params)
        .await
        .expect("modern initialization")
        .capabilities;
    assert!(capabilities.code_action_provider.is_some());
}

async fn open(backend: &Backend, path: &Path, source: &str) -> Url {
    let uri = Url::from_file_path(path).expect("file URI");
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "java".to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;
    uri
}

fn position_of(source: &str, needle: &str) -> Position {
    let byte_offset = source.find(needle).expect("fixture needle");
    let before = &source[..byte_offset];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    let character = source[line_start..byte_offset].encode_utf16().count() as u32;
    Position::new(line, character)
}

fn text_position(uri: Url, position: Position) -> TextDocumentPositionParams {
    TextDocumentPositionParams::new(TextDocumentIdentifier::new(uri), position)
}

async fn completion_labels(backend: &Backend, uri: Url, position: Position) -> Vec<String> {
    let response = backend
        .completion(CompletionParams {
            text_document_position: text_position(uri, position),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
        .await
        .expect("completion request")
        .expect("completion response");

    let items: Vec<CompletionItem> = match response {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    items.into_iter().map(|item| item.label).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unsaved_utf16_edit_updates_completion_navigation_and_rename() {
    let temp = TempDir::new().expect("temp workspace");
    let java_root = temp.path().join("src/main/java");
    let model_root = java_root.join("model");
    let app_root = java_root.join("app");
    std::fs::create_dir_all(&model_root).expect("model source directory");
    std::fs::create_dir_all(&app_root).expect("app source directory");

    let user_path = model_root.join("User.java");
    let account_path = model_root.join("Account.java");
    let main_path = app_root.join("Main.java");
    std::fs::write(&user_path, USER_SOURCE).expect("User fixture");
    std::fs::write(&account_path, ACCOUNT_SOURCE).expect("Account fixture");
    std::fs::write(&main_path, MAIN_SOURCE).expect("Main fixture");

    let backend = Backend::new(test_client());
    let initialize_params: InitializeParams = serde_json::from_value(serde_json::json!({
        "rootUri": Url::from_directory_path(temp.path()).expect("workspace URI"),
        "capabilities": {
            "workspace": {
                "applyEdit": true,
                "workspaceEdit": { "documentChanges": true }
            }
        }
    }))
    .expect("initialize parameters");
    backend
        .initialize(initialize_params)
        .await
        .expect("initialize backend");
    backend.initialized(InitializedParams {}).await;

    let user_uri = open(&backend, &user_path, USER_SOURCE).await;
    let account_uri = open(&backend, &account_path, ACCOUNT_SOURCE).await;
    let main_uri = open(&backend, &main_path, MAIN_SOURCE).await;

    // Initialization publishes the source, dependency, and JDK indices in the
    // background. Wait for that transaction before exercising the editor
    // workflow so its open-buffer revision is not concurrently restored by
    // the initial bulk scan. Generating a rename proof is read-only; the edit
    // is deliberately not applied.
    let readiness_probe = RenameParams {
        text_document_position: text_position(user_uri, position_of(USER_SOURCE, "displayName()")),
        new_name: "visibleName".to_string(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let readiness_deadline =
        tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    loop {
        match backend.rename(readiness_probe.clone()).await {
            Ok(Some(_)) => break,
            Err(_) if tokio::time::Instant::now() < readiness_deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok(None) if tokio::time::Instant::now() < readiness_deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            result => panic!("workspace indexing did not become ready: {result:?}"),
        }
    }

    let completion_position = position_of(MAIN_SOURCE, "value.displayName();");
    let completion_position = Position::new(
        completion_position.line,
        completion_position.character + "value.".encode_utf16().count() as u32,
    );
    let before = completion_labels(&backend, main_uri.clone(), completion_position).await;
    assert!(
        before.iter().any(|label| label == "displayName"),
        "User completion should include displayName, got {before:?}"
    );
    assert!(
        !before.iter().any(|label| label == "accountNumber"),
        "User completion leaked an Account member: {before:?}"
    );

    let user_position = position_of(MAIN_SOURCE, "User value");
    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: main_uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: Some(Range::new(
                    user_position,
                    Position::new(user_position.line, user_position.character + 4),
                )),
                range_length: Some(4),
                text: "Account".to_string(),
            }],
        })
        .await;

    let after = completion_labels(&backend, main_uri.clone(), completion_position).await;
    assert!(
        after.iter().any(|label| label == "accountNumber"),
        "Account completion should include accountNumber after didChange, got {after:?}"
    );
    assert!(
        !after.iter().any(|label| label == "displayName"),
        "completion retained the stale User receiver type: {after:?}"
    );

    let account_call = position_of(MAIN_SOURCE, "accountNumber();");
    let definition = backend
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: text_position(main_uri.clone(), account_call),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("goto-definition request")
        .expect("accountNumber definition");
    let GotoDefinitionResponse::Scalar(location) = definition else {
        panic!("expected a single accountNumber definition, got {definition:?}");
    };
    assert_eq!(location.uri, account_uri);

    let rename_params = RenameParams {
        text_document_position: text_position(main_uri.clone(), account_call),
        new_name: "accountId".to_string(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let deadline = tokio::time::Instant::now() + super::refactor_harness::INDEX_READINESS_TIMEOUT;
    let edit = loop {
        match backend.rename(rename_params.clone()).await {
            Ok(Some(edit)) => break edit,
            Err(error)
                if error.message.contains("indexing is incomplete")
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Ok(None) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            result => panic!("rename request did not become ready: {result:?}"),
        }
    };
    let Some(DocumentChanges::Edits(changes)) = edit.document_changes else {
        panic!("rename should use version-fenced document changes: {edit:?}");
    };
    let has_account_id_edit = |uri: &Url| {
        changes.iter().any(|document| {
            &document.text_document.uri == uri
                && document.edits.iter().any(|edit| match edit {
                    OneOf::Left(edit) => edit.new_text == "accountId",
                    OneOf::Right(edit) => edit.text_edit.new_text == "accountId",
                })
        })
    };
    assert!(
        has_account_id_edit(&account_uri),
        "rename should edit the Account declaration: {changes:?}"
    );
    assert!(
        has_account_id_edit(&main_uri),
        "rename should edit the unsaved Main call: {changes:?}"
    );
}

#[tokio::test]
async fn range_formatting_rejects_invalid_positions_and_keeps_serving_valid_edits() {
    use tower_lsp::lsp_types::{DocumentRangeFormattingParams, FormattingOptions};

    let temp = TempDir::new().expect("temp workspace");
    let backend = Backend::new(test_client());
    let source = "class Foo { String s = \"😀\";int  value;}";
    let uri = open(&backend, &temp.path().join("Foo.java"), source).await;
    let emoji = source.find('😀').expect("emoji fixture");
    let inside_surrogate = source[..emoji].encode_utf16().count() as u32 + 1;
    let params = |range| DocumentRangeFormattingParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range,
        options: FormattingOptions::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    for range in [
        Range::new(Position::new(0, 0), Position::new(1, 0)),
        Range::new(Position::new(0, 0), Position::new(0, 999)),
        Range::new(Position::new(0, 0), Position::new(0, inside_surrogate)),
        Range::new(Position::new(0, 10), Position::new(0, 5)),
    ] {
        assert!(backend
            .range_formatting(params(range))
            .await
            .expect("format request")
            .is_none());
    }

    let start = source.find("int  value;").expect("field fixture");
    let range = Range::new(
        hyperion_lsp::position_utils::offset_to_position(source, start),
        hyperion_lsp::position_utils::offset_to_position(source, start + "int  value;".len()),
    );
    let edits = backend
        .range_formatting(params(range))
        .await
        .expect("valid request")
        .expect("valid selection produces edits");
    assert_eq!(edits.len(), 1);
    let mut applied = source.to_string();
    for edit in edits.into_iter().rev() {
        hyperion_lsp::position_utils::apply_incremental_change(
            &mut applied,
            edit.range,
            &edit.new_text,
        )
        .expect("editor applies formatting edit");
    }
    assert_eq!(applied, "class Foo { String s = \"😀\";int value;}");
}
