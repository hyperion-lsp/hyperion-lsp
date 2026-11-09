// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::io::AsyncWriteExt;
use tower::Service;
use tower_lsp::jsonrpc::{ErrorCode, Request, Result};
use tower_lsp::lsp_types::{InitializeParams, InitializeResult};
use tower_lsp::{CancellationToken, LanguageServer, LspService, Server};

#[derive(Debug, Default)]
struct ResilienceBackend {
    slow_started: Arc<AtomicBool>,
    cancellation_observed: Arc<AtomicBool>,
}

#[tower_lsp::async_trait]
impl LanguageServer for ResilienceBackend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult::default())
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

// These custom handlers intentionally return futures: the dispatcher must catch
// panics and observe cancellation while polling, including handlers with no yield.
#[allow(clippy::unused_async_trait_impl)]
impl ResilienceBackend {
    fn panic_request(&self) -> impl std::future::Future<Output = Result<i32>> {
        std::future::poll_fn(|_| panic!("deliberate request panic"))
    }

    fn succeeding_request(&self) -> impl std::future::Future<Output = Result<i32>> {
        std::future::ready(Ok(42))
    }

    fn slow_request(&self) -> impl std::future::Future<Output = Result<i32>> + '_ {
        std::future::poll_fn(|_| {
            let cancellation = CancellationToken::current();
            self.slow_started.store(true, Ordering::Release);
            let deadline = Instant::now() + Duration::from_secs(5);

            loop {
                if cancellation.is_cancelled() {
                    self.cancellation_observed.store(true, Ordering::Release);
                    return std::task::Poll::Ready(Err(
                        tower_lsp::jsonrpc::Error::request_cancelled(),
                    ));
                }
                assert!(
                    Instant::now() < deadline,
                    "slow test request never observed cancellation"
                );
                std::hint::spin_loop();
            }
        })
    }
}

async fn initialize(service: &mut LspService<ResilienceBackend>) {
    let request = Request::build("initialize")
        .params(json!({"capabilities": {}}))
        .id(1)
        .finish();
    let response = service
        .call(request)
        .await
        .expect("initialize dispatch failed")
        .expect("initialize returned no response");
    assert!(response.is_ok(), "initialize failed: {response:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn panicking_request_returns_internal_error_and_server_keeps_serving() {
    let (mut service, _socket) = LspService::build(|_| ResilienceBackend::default())
        .custom_method("test/panic", ResilienceBackend::panic_request)
        .custom_method("test/succeed", ResilienceBackend::succeeding_request)
        .finish();
    initialize(&mut service).await;

    let panic_response = service
        .call(Request::build("test/panic").id(2).finish())
        .await
        .expect("panic dispatch escaped the service")
        .expect("panic request returned no response");
    assert_eq!(
        panic_response.error().map(|error| error.code),
        Some(ErrorCode::InternalError)
    );

    let success_response = service
        .call(Request::build("test/succeed").id(3).finish())
        .await
        .expect("subsequent dispatch failed")
        .expect("subsequent request returned no response");
    assert_eq!(success_response.result(), Some(&json!(42)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_interrupts_an_in_flight_cpu_bound_request() {
    let backend = ResilienceBackend::default();
    let slow_started = Arc::clone(&backend.slow_started);
    let cancellation_observed = Arc::clone(&backend.cancellation_observed);
    let (mut service, _socket) = LspService::build(|_| backend)
        .custom_method("test/slow", ResilienceBackend::slow_request)
        .finish();
    initialize(&mut service).await;

    let slow_response = tokio::spawn(service.call(Request::build("test/slow").id(2).finish()));
    tokio::time::timeout(Duration::from_secs(1), async {
        while !slow_started.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("slow request did not start");

    service
        .call(
            Request::build("$/cancelRequest")
                .params(json!({"id": 2}))
                .finish(),
        )
        .await
        .expect("cancel notification dispatch failed");

    let response = tokio::time::timeout(Duration::from_secs(1), slow_response)
        .await
        .expect("cancelled request did not return promptly")
        .expect("request task panicked")
        .expect("request dispatch failed")
        .expect("cancelled request returned no response");
    assert_eq!(
        response.error().map(|error| error.code),
        Some(ErrorCode::RequestCancelled)
    );
    assert!(
        cancellation_observed.load(Ordering::Acquire),
        "CPU-bound handler did not observe its cooperative token"
    );
}

#[tokio::test]
async fn exit_terminates_connection_loop_while_input_remains_open() {
    let (mut client_input, server_input) = tokio::io::duplex(1024);
    let (server_output, _client_output) = tokio::io::duplex(1024);
    let (service, socket) = LspService::new(|_| ResilienceBackend::default());
    let server = tokio::spawn(Server::new(server_input, server_output, socket).serve(service));

    let message = r#"{"jsonrpc":"2.0","method":"exit"}"#;
    client_input
        .write_all(format!("Content-Length: {}\r\n\r\n{message}", message.len()).as_bytes())
        .await
        .expect("failed to write exit notification");

    tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .expect("connection loop stayed blocked on open input")
        .expect("server task panicked");

    // Keep the client side alive through the assertion: termination must come from `exit`,
    // not from EOF.
    drop(client_input);
}
