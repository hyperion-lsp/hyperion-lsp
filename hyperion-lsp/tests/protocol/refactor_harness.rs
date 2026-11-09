// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! A real `Backend` over a scratch workspace on disk, plus a client-side
//! `WorkspaceEdit` applier, for end-to-end refactoring tests. Applying edits
//! to the filesystem (rename ops included) lets tests assert the final file
//! contents rather than the shape of the edit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hyperion_lsp::server::Backend;
use tempfile::TempDir;
use tower_lsp::lsp_types::{
    CodeActionContext, CodeActionOrCommand, CodeActionParams, Diagnostic,
    DidOpenTextDocumentParams, DocumentChangeOperation, DocumentChanges, InitializeParams,
    InitializedParams, OneOf, PartialResultParams, Position, Range, ResourceOp,
    TextDocumentIdentifier, TextDocumentItem, TextEdit, Url, WorkDoneProgressParams, WorkspaceEdit,
};
use tower_lsp::{LanguageServer, LspService};

/// How long a test waits for the background workspace index to become usable.
///
/// Initialization indexes the JDK, the dependencies and the sources in the
/// background, and a loaded machine can take several times longer than an idle
/// one. The budget bounds a stuck server rather than a typical run, so it is
/// deliberately far above the time an idle run needs.
pub const INDEX_READINESS_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(5);
pub struct Harness {
    pub temp: TempDir,
    pub backend: Backend,
    /// `(uri, diagnostics)` of every `publishDiagnostics`, in arrival order.
    pub published: Arc<std::sync::Mutex<Vec<(String, Vec<Diagnostic>)>>>,
    /// Every server-to-client request/notification `(method, params)`.
    pub server_messages: Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>,
    pub capabilities: tower_lsp::lsp_types::ServerCapabilities,
}

type Published = Arc<std::sync::Mutex<Vec<(String, Vec<Diagnostic>)>>>;
type Messages = Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>;

async fn test_client() -> (tower_lsp::Client, Published, Messages) {
    use futures::{SinkExt, StreamExt};
    struct Dummy;

    #[tower_lsp::async_trait]
    impl LanguageServer for Dummy {
        async fn initialize(
            &self,
            _: InitializeParams,
        ) -> tower_lsp::jsonrpc::Result<tower_lsp::lsp_types::InitializeResult> {
            Ok(tower_lsp::lsp_types::InitializeResult::default())
        }

        async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
            Ok(())
        }
    }

    let holder = Arc::new(std::sync::Mutex::new(None));
    let captured = Arc::clone(&holder);
    let (mut _service, socket) = LspService::new(move |client| {
        *captured.lock().expect("client holder") = Some(client.clone());
        Dummy
    });
    let client = holder
        .lock()
        .expect("client holder")
        .take()
        .expect("client captured");
    // The client only forwards notifications once its service completed the
    // initialize handshake (it is a stand-in for the editor's connection).
    {
        use tower::Service;
        use tower_lsp::jsonrpc::Request;
        _service
            .call(
                Request::build("initialize")
                    .params(serde_json::json!({"capabilities": {}}))
                    .id(1)
                    .finish(),
            )
            .await
            .expect("client handshake");
        _service
            .call(
                Request::build("initialized")
                    .params(serde_json::json!({}))
                    .finish(),
            )
            .await
            .expect("client handshake");
    }
    let published = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Arc::clone(&published);
    let messages: Messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let message_sink = Arc::clone(&messages);
    let (mut requests, mut responses) = socket.split();
    tokio::spawn(async move {
        // Keep the service alive for as long as the client is used.
        let _service = _service;
        while let Some(request) = requests.next().await {
            message_sink.lock().expect("messages").push((
                request.method().to_string(),
                request.params().cloned().unwrap_or(serde_json::Value::Null),
            ));
            if request.method() == "textDocument/publishDiagnostics" {
                if let Some(params) = request.params().cloned() {
                    if let Ok(params) = serde_json::from_value::<
                        tower_lsp::lsp_types::PublishDiagnosticsParams,
                    >(params)
                    {
                        sink.lock()
                            .expect("published")
                            .push((params.uri.to_string(), params.diagnostics));
                    }
                }
            }
            if let Some(id) = request.id() {
                let result = if request.method() == "workspace/applyEdit" {
                    serde_json::json!({ "applied": true })
                } else {
                    serde_json::Value::Null
                };
                let _ = responses
                    .send(tower_lsp::jsonrpc::Response::from_ok(id.clone(), result))
                    .await;
            }
        }
    });
    (client, published, messages)
}

impl Harness {
    /// Files are `(path relative to src/main/java, content)`.
    pub async fn start(files: &[(&str, &str)]) -> Self {
        Self::start_with_resource_operations(files, &["create", "rename", "delete"]).await
    }

    /// Like [`Self::start`] for a client that supports only some resource operations.
    pub async fn start_with_resource_operations(
        files: &[(&str, &str)],
        resource_operations: &[&str],
    ) -> Self {
        let temp = TempDir::new().expect("temp workspace");
        for (relative, content) in files {
            let path = temp.path().join("src/main/java").join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, content).expect("fixture");
        }
        let (client, published, server_messages) = test_client().await;
        let backend = Backend::new(client);
        let params: InitializeParams = serde_json::from_value(serde_json::json!({
            "rootUri": Url::from_directory_path(temp.path()).expect("workspace URI"),
            "capabilities": {
                "textDocument": {
                    "codeAction": { "codeActionLiteralSupport": { "codeActionKind": { "valueSet": [
                        "quickfix", "refactor", "refactor.extract", "refactor.move", "source",
                        "source.organizeImports"
                    ] } } }
                },
                "workspace": {
                    "applyEdit": true,
                    "workspaceEdit": {
                        "documentChanges": true,
                        "resourceOperations": resource_operations
                    },
                    "fileOperations": { "willRename": true, "didRename": true },
                    "didChangeWatchedFiles": { "dynamicRegistration": true }
                }
            }
        }))
        .expect("initialize parameters");
        let capabilities = backend
            .initialize(params)
            .await
            .expect("initialize")
            .capabilities;
        backend.initialized(InitializedParams {}).await;
        Self {
            temp,
            backend,
            published,
            server_messages,
            capabilities,
        }
    }

    /// Diagnostic messages of the latest publication for `relative`.
    pub fn latest_diagnostics(&self, relative: &str) -> Option<Vec<String>> {
        let uri = self.uri(relative).to_string();
        self.published
            .lock()
            .expect("published")
            .iter()
            .rev()
            .find(|(published, _)| *published == uri)
            .map(|(_, diagnostics)| diagnostics.iter().map(|d| d.message.clone()).collect())
    }

    /// The latest published diagnostics themselves.
    pub fn latest_diagnostic_items(&self, relative: &str) -> Option<Vec<Diagnostic>> {
        let uri = self.uri(relative).to_string();
        self.published
            .lock()
            .expect("published")
            .iter()
            .rev()
            .find(|(published, _)| *published == uri)
            .map(|(_, diagnostics)| diagnostics.clone())
    }

    /// Code actions for `diagnostic` (as an editor requests them at its range).
    pub async fn quick_fixes(
        &self,
        relative: &str,
        diagnostic: &Diagnostic,
    ) -> Vec<CodeActionOrCommand> {
        self.backend
            .code_action(CodeActionParams {
                text_document: TextDocumentIdentifier::new(self.uri(relative)),
                range: diagnostic.range,
                context: CodeActionContext {
                    diagnostics: vec![diagnostic.clone()],
                    only: None,
                    trigger_kind: None,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("code action request")
            .unwrap_or_default()
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.temp.path().join("src/main/java").join(relative)
    }

    pub fn uri(&self, relative: &str) -> Url {
        Url::from_file_path(self.path(relative)).expect("file URI")
    }

    pub fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.path(relative))
            .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
    }

    pub fn exists(&self, relative: &str) -> bool {
        self.path(relative).exists()
    }

    pub async fn open(&self, relative: &str) -> Url {
        let uri = self.uri(relative);
        let language = if std::path::Path::new(relative)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("kt"))
        {
            "kotlin"
        } else {
            "java"
        };
        self.backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: language.to_string(),
                    version: 1,
                    text: self.read(relative),
                },
            })
            .await;
        uri
    }

    /// Position of the first occurrence of `needle` (plus `delta` columns).
    pub fn position_of(&self, relative: &str, needle: &str, delta: u32) -> Position {
        let source = self.read(relative);
        let offset = source.find(needle).expect("needle in fixture");
        let before = &source[..offset];
        let line = before.matches('\n').count() as u32;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let character = source[line_start..offset].encode_utf16().count() as u32 + delta;
        Position::new(line, character)
    }

    /// Request code actions at `position`, retrying until `ready` accepts the
    /// list (the workspace index is built in the background).
    pub async fn code_actions_until(
        &self,
        relative: &str,
        position: Position,
        ready: impl Fn(&[CodeActionOrCommand]) -> bool,
    ) -> Vec<CodeActionOrCommand> {
        let deadline = tokio::time::Instant::now() + INDEX_READINESS_TIMEOUT;
        loop {
            let actions = self
                .backend
                .code_action(CodeActionParams {
                    text_document: TextDocumentIdentifier::new(self.uri(relative)),
                    range: Range::new(position, position),
                    context: CodeActionContext::default(),
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                })
                .await
                .expect("code action request")
                .unwrap_or_default();
            if ready(&actions) {
                return actions;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "code actions never became ready; last: {:?}",
                actions.iter().map(title_of).collect::<Vec<_>>()
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}

pub fn title_of(action: &CodeActionOrCommand) -> String {
    match action {
        CodeActionOrCommand::CodeAction(action) => action.title.clone(),
        CodeActionOrCommand::Command(command) => command.title.clone(),
    }
}

pub fn find_action<'a>(
    actions: &'a [CodeActionOrCommand],
    title: &str,
) -> Option<&'a tower_lsp::lsp_types::CodeAction> {
    actions.iter().find_map(|action| match action {
        CodeActionOrCommand::CodeAction(action) if action.title == title => Some(action),
        _ => None,
    })
}

fn apply_text_edits(text: &str, edits: &[TextEdit]) -> String {
    let offset_of = |position: Position| -> usize {
        let mut line_start = 0;
        for _ in 0..position.line {
            line_start += text[line_start..]
                .find('\n')
                .map_or(text.len() - line_start, |i| i + 1);
        }
        let mut units = 0u32;
        for (index, ch) in text[line_start..].char_indices() {
            if units >= position.character || ch == '\n' {
                return line_start + index;
            }
            units += ch.len_utf16() as u32;
        }
        text.len()
    };
    let mut ordered = edits.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|edit| (edit.range.start.line, edit.range.start.character));
    let mut out = text.to_string();
    for edit in ordered.into_iter().rev() {
        out.replace_range(
            offset_of(edit.range.start)..offset_of(edit.range.end),
            &edit.new_text,
        );
    }
    out
}

fn edit_file(path: &Path, edits: &[TextEdit]) {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("edit target {} unreadable: {error}", path.display()));
    std::fs::write(path, apply_text_edits(&text, edits)).expect("write edited file");
}

/// Apply a workspace edit to the filesystem the way a client would.
pub fn apply_workspace_edit(edit: &WorkspaceEdit) {
    if let Some(document_changes) = &edit.document_changes {
        let operations: Vec<DocumentChangeOperation> = match document_changes {
            DocumentChanges::Edits(edits) => edits
                .iter()
                .cloned()
                .map(DocumentChangeOperation::Edit)
                .collect(),
            DocumentChanges::Operations(ops) => ops.clone(),
        };
        for operation in operations {
            match operation {
                DocumentChangeOperation::Edit(document) => {
                    let edits = document
                        .edits
                        .iter()
                        .map(|edit| match edit {
                            OneOf::Left(edit) => edit.clone(),
                            OneOf::Right(annotated) => annotated.text_edit.clone(),
                        })
                        .collect::<Vec<_>>();
                    edit_file(&document.text_document.uri.to_file_path().unwrap(), &edits);
                }
                DocumentChangeOperation::Op(ResourceOp::Rename(rename)) => {
                    let new = rename.new_uri.to_file_path().unwrap();
                    std::fs::create_dir_all(new.parent().unwrap()).unwrap();
                    std::fs::rename(rename.old_uri.to_file_path().unwrap(), new).unwrap();
                }
                DocumentChangeOperation::Op(ResourceOp::Create(create)) => {
                    let path = create.uri.to_file_path().unwrap();
                    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                    std::fs::write(path, "").unwrap();
                }
                DocumentChangeOperation::Op(ResourceOp::Delete(delete)) => {
                    std::fs::remove_file(delete.uri.to_file_path().unwrap()).unwrap();
                }
            }
        }
    } else if let Some(changes) = &edit.changes {
        let changes: &HashMap<Url, Vec<TextEdit>> = changes;
        for (uri, edits) in changes {
            edit_file(&uri.to_file_path().unwrap(), edits);
        }
    }
}
