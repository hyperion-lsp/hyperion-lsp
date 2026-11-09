// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Per-language dispatch for `textDocument/signatureHelp` and
//! `textDocument/inlayHint`, exercised through the real LSP request path.
//!
//! These go through `LanguageServer::signature_help` / `inlay_hint` on a live
//! `Backend` — `did_open` first, so the server derives the language from the
//! document URI exactly as an editor would.
//!
//! Half the tests here are Java: the opt-in `LanguageHandler` convention means
//! Java must keep falling through to its own provider, and these pin that the
//! Java answers are byte-for-byte what the Java providers produce on their own.

use hyperion_lsp::server::Backend;
use hyperion_semantics::db::{FileId, SourceDatabase};
use hyperion_workspace::WorkspaceIndex;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use tower_lsp::lsp_types::{
    DidOpenTextDocumentParams, InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams, Position,
    Range, SignatureHelp, SignatureHelpContext, SignatureHelpParams, SignatureHelpTriggerKind,
    TextDocumentIdentifier, TextDocumentItem, TextDocumentPositionParams, Url,
    WorkDoneProgressParams,
};
use tower_lsp::{LanguageServer, LspService};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

const CURSOR: &str = "<|>";

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

struct OpenDocument {
    _temp: TempDir,
    backend: Backend,
    uri: Url,
    text: String,
    cursor: Position,
}

/// Open `source` (cursor marked with `<|>`) as `file_name` on a live `Backend`.
async fn open(file_name: &str, source: &str) -> OpenDocument {
    let temp = TempDir::new().expect("temp dir");
    let path = temp.path().join(file_name);

    let cursor_offset = source.find(CURSOR);
    let text = source.replace(CURSOR, "");
    std::fs::write(&path, &text).expect("write source");

    let uri = Url::from_file_path(&path).expect("file uri");
    let backend = Backend::new(test_client());

    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: language_id(file_name).to_string(),
                version: 1,
                text: text.clone(),
            },
        })
        .await;

    let cursor = cursor_offset
        .map(|offset| hyperion_lsp::position_utils::offset_to_position(&text, offset))
        .unwrap_or_default();

    OpenDocument {
        _temp: temp,
        backend,
        uri,
        text,
        cursor,
    }
}

fn language_id(file_name: &str) -> &'static str {
    match std::path::Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
    {
        Some("kt") => "kotlin",
        _ => "java",
    }
}

impl OpenDocument {
    async fn signature_help(&self) -> Option<SignatureHelp> {
        self.backend
            .signature_help(SignatureHelpParams {
                context: Some(SignatureHelpContext {
                    trigger_kind: SignatureHelpTriggerKind::TRIGGER_CHARACTER,
                    trigger_character: Some("(".to_string()),
                    is_retrigger: false,
                    active_signature_help: None,
                }),
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: self.uri.clone(),
                    },
                    position: self.cursor,
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("signature help request must not fail")
    }

    async fn inlay_hints(&self) -> Vec<InlayHint> {
        self.backend
            .inlay_hint(InlayHintParams {
                text_document: TextDocumentIdentifier {
                    uri: self.uri.clone(),
                },
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: hyperion_lsp::position_utils::offset_to_position(
                        &self.text,
                        self.text.len(),
                    ),
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .expect("inlay hint request must not fail")
            .unwrap_or_default()
    }
}

fn hint_labels(hints: &[InlayHint], kind: InlayHintKind) -> Vec<String> {
    hints
        .iter()
        .filter(|hint| hint.kind == Some(kind))
        .map(|hint| match &hint.label {
            InlayHintLabel::String(text) => text.clone(),
            InlayHintLabel::LabelParts(parts) => parts.iter().map(|p| p.value.clone()).collect(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Kotlin routes to the Kotlin providers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kotlin_signature_help_through_the_request_path() {
    let doc = open(
        "Main.kt",
        r#"
fun greet(name: String, times: Int): String = ""

fun main() {
    greet("bob", <|>)
}
"#,
    )
    .await;

    let help = doc
        .signature_help()
        .await
        .expect("Kotlin signature help must reach the Kotlin provider");

    assert_eq!(
        help.signatures[0].label,
        "greet(name: String, times: Int): String"
    );
    assert_eq!(help.active_parameter, Some(1));
}

#[tokio::test]
async fn kotlin_inlay_hints_through_the_request_path() {
    let doc = open(
        "Main.kt",
        r#"
fun greet(name: String, times: Int) {}

fun main() {
    val label = "bob"
    greet("alice", 3)
}
"#,
    )
    .await;

    let hints = doc.inlay_hints().await;

    assert_eq!(hint_labels(&hints, InlayHintKind::TYPE), vec![": String"]);
    assert_eq!(
        hint_labels(&hints, InlayHintKind::PARAMETER),
        vec!["name = ", "times = "]
    );
}

#[tokio::test]
async fn kotlin_hints_are_absent_outside_a_call() {
    let doc = open("Main.kt", "fun main() {\n    val x = 1<|>\n}\n").await;
    assert!(doc.signature_help().await.is_none());
}

// ---------------------------------------------------------------------------
// Java still falls through, unchanged
// ---------------------------------------------------------------------------

/// The Java signature-help answer, computed straight from the Java provider
/// with no dispatch layer in between.
fn java_signature_help_reference(source: &str, cursor: Position) -> Option<SignatureHelp> {
    use hyperion_semantics::db::SourceDatabase;

    let index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp/hyperion-pin")));
    let host = hyperion_lsp::semantic_host::SemanticHost::new(Arc::clone(&index));
    let file_id = FileId::new(0);
    host.with_db(|guard| {
        guard.set_file_text(file_id, Arc::new(source.to_string()));
        guard.set_file_language(file_id, hyperion_workspace::Language::Java);
    });
    let txn = host.read_now();
    hyperion_lsp::signature_help::SignatureHelpProvider::new(&txn, index)
        .signature_help(FileId::new(0), cursor, source)
        .expect("java signature help")
}

/// The Java inlay-hint answer, computed straight from the Java provider.
fn java_inlay_hints_reference(source: &str) -> Vec<InlayHint> {
    let index = Arc::new(WorkspaceIndex::new(PathBuf::from("/tmp/hyperion-pin")));
    let host = hyperion_lsp::semantic_host::SemanticHost::new(Arc::clone(&index));
    let file_id = FileId::new(0);
    host.with_db(|guard| {
        guard.set_file_text(file_id, Arc::new(source.to_string()));
        guard.set_file_language(file_id, hyperion_workspace::Language::Java);
    });
    let range = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: hyperion_lsp::position_utils::offset_to_position(source, source.len()),
    };
    let txn = host.read_now();
    hyperion_lsp::inlay_hints::InlayHintsProvider::new(&txn, index)
        .inlay_hints(file_id, range, source)
        .expect("java inlay hints")
}

#[tokio::test]
async fn java_signature_help_is_unchanged_by_dispatch() {
    let doc = open(
        "Widget.java",
        r"public class Widget {
    void build() {
        Widget w = new Widget(<|>);
    }
}
",
    )
    .await;

    let through_server = doc.signature_help().await;
    let direct = java_signature_help_reference(&doc.text, doc.cursor);

    assert!(
        direct.is_some(),
        "the reference must be non-empty or this pin is vacuous"
    );
    assert_eq!(
        format!("{through_server:?}"),
        format!("{direct:?}"),
        "Java signature help must be byte-for-byte what the Java provider returns"
    );

    let help = through_server.expect("java signature help");
    assert_eq!(help.signatures[0].label, "Widget()");
}

#[tokio::test]
async fn java_inlay_hints_are_unchanged_by_dispatch() {
    let doc = open(
        "Counter.java",
        r#"public class Counter {
    void run() {
        var count = 5;
        var label = "hello";
    }
}
"#,
    )
    .await;

    let through_server = doc.inlay_hints().await;
    let direct = java_inlay_hints_reference(&doc.text);

    assert!(
        !direct.is_empty(),
        "the reference must be non-empty or this pin is vacuous"
    );
    assert_eq!(
        format!("{through_server:?}"),
        format!("{direct:?}"),
        "Java inlay hints must be byte-for-byte what the Java provider returns"
    );

    assert_eq!(
        hint_labels(&through_server, InlayHintKind::TYPE),
        vec![": int", ": String"]
    );
}
