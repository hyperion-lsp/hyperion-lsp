// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::time::Duration;

use futures::StreamExt;
use hyperion_lsp::server::create_lsp_server;
use serde_json::json;
use tower::Service;
use tower_lsp::jsonrpc::Request;

#[tokio::test]
async fn dynamically_registers_type_hierarchy_for_supporting_clients() {
    let (mut service, mut socket) = create_lsp_server();
    let initialize = Request::build("initialize")
        .params(json!({
            "capabilities": {
                "textDocument": {
                    "typeHierarchy": { "dynamicRegistration": true }
                }
            }
        }))
        .id(1)
        .finish();
    let response = service
        .call(initialize)
        .await
        .expect("initialize dispatch")
        .expect("initialize response");
    assert!(response.is_ok(), "initialize failed: {response:?}");

    let initialized = service.call(Request::build("initialized").params(json!({})).finish());
    tokio::pin!(initialized);

    let registration = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            tokio::select! {
                response = &mut initialized => {
                    response.expect("initialized dispatch");
                    panic!("initialized completed before registering type hierarchy");
                }
                request = socket.next() => {
                    let request = request.expect("client socket remains open");
                    if request.method() == "client/registerCapability" {
                        break request;
                    }
                }
            }
        }
    })
    .await
    .expect("type hierarchy registration was not sent");

    assert_eq!(
        registration
            .params()
            .and_then(|params| params.pointer("/registrations/0/method")),
        Some(&json!("textDocument/prepareTypeHierarchy"))
    );
    assert_eq!(
        registration
            .params()
            .and_then(|params| params.pointer("/registrations/0/registerOptions/documentSelector")),
        Some(&json!([
            { "language": "java" },
            { "language": "kotlin" }
        ]))
    );
}
