// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Behavioral contracts for language-gated LSP requests.
//!
//! These exercise the real `LanguageServer` boundary. The fixture is
//! intentionally Java-shaped even when opened as Kotlin: before the gate was
//! added, the Java parser returned plausible but meaningless editor results.

use std::path::Path;
use std::sync::Arc;

use hyperion_lsp::server::Backend;
use tempfile::TempDir;
use tower_lsp::lsp_types::{
    CodeActionContext, CodeActionParams, CodeLensParams, DidOpenTextDocumentParams,
    DocumentLinkParams, DocumentSymbolParams, FoldingRangeParams, PartialResultParams, Position,
    Range, TextDocumentIdentifier, TextDocumentItem, Url, WorkDoneProgressParams,
};
use tower_lsp::{LanguageServer, LspService};

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

async fn open(backend: &Backend, path: &Path, language_id: &str, source: &str) -> Url {
    std::fs::write(path, source).expect("write source fixture");
    let uri = Url::from_file_path(path).expect("file URI");
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: language_id.to_string(),
                version: 1,
                text: source.to_string(),
            },
        })
        .await;
    uri
}

fn folding_params(uri: Url) -> FoldingRangeParams {
    FoldingRangeParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn link_params(uri: Url) -> DocumentLinkParams {
    DocumentLinkParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn lens_params(uri: Url) -> CodeLensParams {
    CodeLensParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

fn action_params(uri: Url) -> CodeActionParams {
    CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::new(Position::new(1, 4), Position::new(1, 4)),
        context: CodeActionContext {
            diagnostics: Vec::new(),
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    }
}

#[tokio::test]
async fn java_tree_features_answer_java_and_decline_kotlin() {
    let temp = TempDir::new().expect("temp workspace");
    let backend = Backend::new(test_client());
    let source = r"public class Main {
    private String name;
    // See https://example.com/docs
    public static void main(String[] args) {
    }
}
";

    let java_uri = open(&backend, &temp.path().join("Main.java"), "java", source).await;
    let kotlin_uri = open(&backend, &temp.path().join("Main.kt"), "kotlin", source).await;

    let java_folds = backend
        .folding_range(folding_params(java_uri.clone()))
        .await
        .expect("Java folding request");
    assert!(java_folds.is_some_and(|ranges| !ranges.is_empty()));

    let java_links = backend
        .document_link(link_params(java_uri.clone()))
        .await
        .expect("Java document-link request");
    assert!(java_links.is_some_and(|links| !links.is_empty()));

    let java_lenses = backend
        .code_lens(lens_params(java_uri.clone()))
        .await
        .expect("Java code-lens request");
    assert!(java_lenses.is_some_and(|lenses| !lenses.is_empty()));

    let java_actions = backend
        .code_action(action_params(java_uri))
        .await
        .expect("Java code-action request");
    assert!(java_actions.is_some_and(|actions| !actions.is_empty()));

    assert!(backend
        .folding_range(folding_params(kotlin_uri.clone()))
        .await
        .expect("Kotlin folding request")
        .is_none());
    assert!(backend
        .document_link(link_params(kotlin_uri.clone()))
        .await
        .expect("Kotlin document-link request")
        .is_none());
    assert!(backend
        .code_lens(lens_params(kotlin_uri.clone()))
        .await
        .expect("Kotlin code-lens request")
        .is_none());
    assert!(backend
        .code_action(action_params(kotlin_uri))
        .await
        .expect("Kotlin code-action request")
        .is_none());
}

#[tokio::test]
async fn unsupported_extension_is_ignored_even_when_client_calls_it_java() {
    let temp = TempDir::new().expect("temp workspace");
    let backend = Backend::new(test_client());
    let uri = open(
        &backend,
        &temp.path().join("NotJava.txt"),
        "java",
        "public class NotJava {}",
    )
    .await;

    let symbols = backend
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("document-symbol request");

    assert!(symbols.is_none());
}
