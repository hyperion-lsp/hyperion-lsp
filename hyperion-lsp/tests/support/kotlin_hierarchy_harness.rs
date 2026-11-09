// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Shared harness for the call- and type-hierarchy request tests.
//!
//! Everything here drives a live [`Backend`] through the real
//! `LanguageServer` methods, so language dispatch is exercised exactly as an
//! editor would exercise it: `did_open` first, then the request.
//!
//! Files are opened in *dependency order* on purpose. `did_open` is what indexes
//! a file, and the usage extractor only records a reference it can resolve
//! against the index — so a callee's file has to be opened before its caller's,
//! the same way a workspace scan reaches declarations before it reaches uses.

#![allow(dead_code)]

use hyperion_lsp::server::Backend;
use std::sync::Arc;
use tempfile::TempDir;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    DidOpenTextDocumentParams, GotoDefinitionParams, GotoDefinitionResponse, PartialResultParams,
    Position, TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams,
    TypeHierarchyItem, TypeHierarchyPrepareParams, TypeHierarchySubtypesParams,
    TypeHierarchySupertypesParams, Url, WorkDoneProgressParams,
};
use tower_lsp::{LanguageServer, LspService};

/// Cursor marker recognised in fixture sources.
pub const CURSOR: &str = "<|>";

/// A `tower_lsp::Client` wired to a do-nothing server, so a `Backend` can be
/// built outside a real connection.
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
    let holder_clone = Arc::clone(&holder);
    let (_service, _socket) = LspService::new(move |client| {
        *holder_clone.lock().expect("client holder") = Some(client.clone());
        Dummy
    });

    let client = holder
        .lock()
        .expect("client holder")
        .take()
        .expect("client captured");
    client
}

/// One opened document in the workspace.
pub struct OpenFile {
    pub uri: Url,
    pub text: String,
    pub cursor: Position,
}

/// A workspace of opened documents on a live `Backend`.
pub struct Workspace {
    _temp: TempDir,
    pub backend: Backend,
    files: Vec<(String, OpenFile)>,
}

impl Workspace {
    /// Open `files` in the order given. Each entry is `(name, source)`; a `<|>`
    /// marker in the source records the cursor for that file.
    pub async fn open(files: &[(&str, &str)]) -> Self {
        let temp = TempDir::new().expect("temp dir");
        let backend = Backend::new(test_client());
        let mut opened = Vec::new();

        for (name, source) in files {
            let path = temp.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create fixture dirs");
            }

            let cursor_offset = source.find(CURSOR);
            let text = source.replace(CURSOR, "");
            std::fs::write(&path, &text).expect("write source");

            let uri = Url::from_file_path(&path).expect("file uri");
            backend
                .did_open(DidOpenTextDocumentParams {
                    text_document: TextDocumentItem {
                        uri: uri.clone(),
                        language_id: language_id(name).to_string(),
                        version: 1,
                        text: text.clone(),
                    },
                })
                .await;

            let cursor = cursor_offset
                .map(|offset| hyperion_lsp::position_utils::offset_to_position(&text, offset))
                .unwrap_or_default();

            opened.push(((*name).to_string(), OpenFile { uri, text, cursor }));
        }

        Self {
            _temp: temp,
            backend,
            files: opened,
        }
    }

    pub fn file(&self, name: &str) -> &OpenFile {
        self.files
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map_or_else(|| panic!("no such fixture file: {name}"), |(_, file)| file)
    }

    // -- call hierarchy ----------------------------------------------------

    /// `prepareCallHierarchy` at the file's cursor marker.
    pub async fn prepare_call_hierarchy(&self, name: &str) -> Option<Vec<CallHierarchyItem>> {
        let file = self.file(name);
        self.backend
            .prepare_call_hierarchy(CallHierarchyPrepareParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: file.uri.clone(),
                    },
                    position: file.cursor,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("prepare call hierarchy must not fail")
    }

    /// `prepareCallHierarchy` at an explicit position, for fixtures that carry
    /// no cursor marker (a Java file whose Kotlin counterpart owns the marker).
    pub async fn backend_prepare_at(
        &self,
        name: &str,
        line: u32,
        character: u32,
    ) -> Option<Vec<CallHierarchyItem>> {
        let file = self.file(name);
        self.backend
            .prepare_call_hierarchy(CallHierarchyPrepareParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: file.uri.clone(),
                    },
                    position: Position::new(line, character),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("prepare call hierarchy must not fail")
    }

    /// The single item `prepareCallHierarchy` is expected to return.
    pub async fn call_hierarchy_item(&self, name: &str) -> CallHierarchyItem {
        let items = self
            .prepare_call_hierarchy(name)
            .await
            .unwrap_or_else(|| panic!("expected a call hierarchy item in {name}"));
        assert_eq!(items.len(), 1, "expected exactly one item, got {items:?}");
        items.into_iter().next().expect("one item")
    }

    pub async fn incoming_calls(&self, item: CallHierarchyItem) -> Vec<CallHierarchyIncomingCall> {
        self.backend
            .incoming_calls(CallHierarchyIncomingCallsParams {
                item,
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("incoming calls must not fail")
            .unwrap_or_default()
    }

    pub async fn outgoing_calls(&self, item: CallHierarchyItem) -> Vec<CallHierarchyOutgoingCall> {
        self.backend
            .outgoing_calls(CallHierarchyOutgoingCallsParams {
                item,
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("outgoing calls must not fail")
            .unwrap_or_default()
    }

    // -- type hierarchy ----------------------------------------------------

    /// `prepareTypeHierarchy` at the file's cursor marker.
    pub async fn prepare_type_hierarchy(&self, name: &str) -> Option<Vec<TypeHierarchyItem>> {
        let file = self.file(name);
        self.backend
            .prepare_type_hierarchy(TypeHierarchyPrepareParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: file.uri.clone(),
                    },
                    position: file.cursor,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("prepare type hierarchy must not fail")
    }

    /// `prepareTypeHierarchy` at an explicit position, for fixtures that carry
    /// no cursor marker.
    pub async fn prepare_type_hierarchy_at(
        &self,
        name: &str,
        line: u32,
        character: u32,
    ) -> Option<Vec<TypeHierarchyItem>> {
        let file = self.file(name);
        self.backend
            .prepare_type_hierarchy(TypeHierarchyPrepareParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: file.uri.clone(),
                    },
                    position: Position::new(line, character),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("prepare type hierarchy must not fail")
    }

    /// The single item `prepareTypeHierarchy` is expected to return.
    pub async fn type_hierarchy_item(&self, name: &str) -> TypeHierarchyItem {
        let items = self
            .prepare_type_hierarchy(name)
            .await
            .unwrap_or_else(|| panic!("expected a type hierarchy item in {name}"));
        assert_eq!(items.len(), 1, "expected exactly one item, got {items:?}");
        items.into_iter().next().expect("one item")
    }

    pub async fn supertypes(&self, item: TypeHierarchyItem) -> Vec<TypeHierarchyItem> {
        self.backend
            .supertypes(TypeHierarchySupertypesParams {
                item,
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("supertypes must not fail")
            .unwrap_or_default()
    }

    pub async fn subtypes(&self, item: TypeHierarchyItem) -> Vec<TypeHierarchyItem> {
        self.backend
            .subtypes(TypeHierarchySubtypesParams {
                item,
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("subtypes must not fail")
            .unwrap_or_default()
    }

    pub async fn goto_implementation(&self, name: &str) -> Option<GotoDefinitionResponse> {
        let file = self.file(name);
        self.backend
            .goto_implementation(GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: file.uri.clone(),
                    },
                    position: file.cursor,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            })
            .await
            .expect("goto implementation must not fail")
    }
}

fn language_id(file_name: &str) -> &'static str {
    match std::path::Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("kt") => "kotlin",
        _ => "java",
    }
}

/// Names of the callers in an incoming-calls response, sorted.
pub fn caller_names(calls: &[CallHierarchyIncomingCall]) -> Vec<String> {
    let mut names: Vec<String> = calls.iter().map(|call| call.from.name.clone()).collect();
    names.sort();
    names
}

/// Names of the callees in an outgoing-calls response, sorted.
pub fn callee_names(calls: &[CallHierarchyOutgoingCall]) -> Vec<String> {
    let mut names: Vec<String> = calls.iter().map(|call| call.to.name.clone()).collect();
    names.sort();
    names
}

/// `detail` (the FQN) of each type-hierarchy item, sorted.
pub fn type_fqns(items: &[TypeHierarchyItem]) -> Vec<String> {
    let mut fqns: Vec<String> = items
        .iter()
        .map(|item| item.detail.clone().unwrap_or_else(|| item.name.clone()))
        .collect();
    fqns.sort();
    fqns
}

/// The text a range covers, for asserting that call-site ranges point at the
/// call and not at the enclosing declaration.
pub fn text_at(source: &str, range: tower_lsp::lsp_types::Range) -> String {
    let start = hyperion_lsp::position_utils::position_to_offset(source, range.start)
        .expect("range start in bounds");
    let end = hyperion_lsp::position_utils::position_to_offset(source, range.end)
        .expect("range end in bounds");
    source[start..end].to_string()
}
