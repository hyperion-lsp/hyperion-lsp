// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end regression coverage for no-workspace-root initialization
//! These tests drive the real JSON-RPC service because the bug
//! was in lifecycle state shared by `initialized`, `didOpen`, diagnostics,
//! and lazy source-root indexing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use hyperion_lsp::server::create_lsp_server;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tower::Service;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::Url;

#[tokio::test]
async fn rootless_initialization_enables_analysis_and_background_indexing() {
    let temp = tempfile::tempdir().expect("temporary source tree");
    let source_root = temp.path().join("src/main/java/example");
    std::fs::create_dir_all(&source_root).expect("create Java source root");

    let open_path = source_root.join("OpenFile.java");
    let sibling_path = source_root.join("Sibling.java");
    let open_source = r#"
        package example;
        class OpenFile {
            MissingType value;
        }
    "#;
    std::fs::write(&open_path, open_source).expect("write open source");
    std::fs::write(
        &sibling_path,
        "package example; public class RootlessSibling {}",
    )
    .expect("write sibling source");

    let (mut service, socket) = create_lsp_server();
    let messages = Arc::new(Mutex::new(Vec::<Request>::new()));
    let messages_for_drain = Arc::clone(&messages);
    tokio::spawn(async move {
        let mut socket = socket;
        while let Some(request) = socket.next().await {
            messages_for_drain.lock().await.push(request);
        }
    });

    let initialize = Request::build("initialize")
        .params(json!({ "capabilities": {} }))
        .id(1)
        .finish();
    let response = service
        .call(initialize)
        .await
        .expect("initialize dispatch")
        .expect("initialize response");
    assert!(response.is_ok(), "initialize failed: {response:?}");

    service
        .call(Request::build("initialized").params(json!({})).finish())
        .await
        .expect("initialized dispatch");

    let uri = Url::from_file_path(&open_path).expect("file URI");
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": "java",
                        "version": 1,
                        "text": open_source
                    }
                }))
                .finish(),
        )
        .await
        .expect("didOpen dispatch");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut request_id = 2;
    let mut found_sibling = false;
    let mut found_semantic_diagnostic = false;
    while Instant::now() < deadline {
        let response = service
            .call(
                Request::build("workspace/symbol")
                    .params(json!({ "query": "RootlessSibling" }))
                    .id(request_id)
                    .finish(),
            )
            .await
            .expect("workspace/symbol dispatch")
            .expect("workspace/symbol response");
        request_id += 1;

        found_sibling = response.result().is_some_and(|result| {
            result.as_array().is_some_and(|symbols| {
                symbols
                    .iter()
                    .any(|symbol| symbol.get("name") == Some(&json!("RootlessSibling")))
            })
        });
        if found_sibling {
            found_semantic_diagnostic = messages.lock().await.iter().any(|request| {
                request.method() == "textDocument/publishDiagnostics"
                    && request
                        .params()
                        .and_then(|params| params.get("diagnostics"))
                        .and_then(Value::as_array)
                        .is_some_and(|diagnostics| {
                            diagnostics.iter().any(|diagnostic| {
                                diagnostic.get("code") == Some(&json!("undefined-symbol"))
                            })
                        })
            });
            if found_semantic_diagnostic {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let captured = messages.lock().await;
    assert!(
        captured.iter().any(|request| {
            request.method() == "window/logMessage"
                && request
                    .params()
                    .and_then(|params| params.get("message"))
                    .and_then(Value::as_str)
                    .is_some_and(|message| message.contains("Indexing JDK stdlib"))
        }),
        "no-root initialized must start root-independent JDK indexing; messages: {captured:?}"
    );
    assert!(
        found_semantic_diagnostic,
        "no-root didOpen must publish semantic diagnostics immediately; messages: {captured:?}"
    );
    assert!(
        found_sibling,
        "no-root didOpen must schedule source-root indexing before an interactive request"
    );
}
