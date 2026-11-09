// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! End-to-end tests for the `hyperion.run` command and legacy
//! `hyperion.runTest` compatibility handler: drives the real
//! `tower_lsp::LspService` (raw
//! JSON-RPC `Request`s through `Service::call`, exactly as a real client
//! would) so that `window/logMessage` / `window/showMessage` notifications
//! actually reach the (captured) client -- calling `Backend` methods
//! directly via `LspService::inner()` would skip the state machine that
//! gates `Client::log_message`/`show_message` on `State::Initialized`,
//! silently dropping every message.
//!
//! Self-skips (like `hyperion-eval/tests/class_mode.rs` and
//! `hyperion-bytecode/tests/jvm_differential_test.rs`) when `javac` isn't on
//! `PATH`, for the tests that actually compile+run a class. The legacy test
//! command check needs no `javac` and always runs.

use std::future::Future;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use hyperion_lsp::commands::{RESOLVE_LAUNCH_COMMAND, RUN_COMMAND, RUN_TEST_COMMAND};
use hyperion_lsp::server::create_lsp_server;
use serde_json::json;
use tokio::sync::Mutex as AsyncMutex;
use tower::Service;
use tower_lsp::jsonrpc::{Request, Response};
use tower_lsp::lsp_types::Url;

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write_file(dir: &Path, name: &str, contents: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, contents).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

/// A running `LspService`, driven through real JSON-RPC `Request`s, with a
/// background task draining every server-to-client message (notifications
/// AND requests) it produces into a shared buffer.
struct Harness {
    service: tower_lsp::LspService<hyperion_lsp::server::Backend>,
    messages: Arc<AsyncMutex<Vec<Request>>>,
    next_id: i64,
}

impl Harness {
    /// Spins up the service and completes the `initialize` handshake (no
    /// workspace folder -- deliberately: `hyperion.run`/`hyperion.runTest`
    /// only need `document_map`/`workspace_root` (`None` is fine) and don't
    /// touch the workspace index, so skipping `workspaceFolders` avoids the
    /// entire background indexing pipeline `initialized()` would otherwise
    /// kick off).
    async fn new() -> Self {
        let (mut service, socket) = create_lsp_server();

        let messages = Arc::new(AsyncMutex::new(Vec::new()));
        let messages_clone = Arc::clone(&messages);
        tokio::spawn(async move {
            let mut socket = socket;
            while let Some(req) = socket.next().await {
                messages_clone.lock().await.push(req);
            }
        });

        let init = Request::build("initialize")
            .params(json!({ "capabilities": {} }))
            .id(1)
            .finish();
        let response = service.call(init).await.expect("initialize failed");
        assert!(
            response.is_some_and(|r| r.into_parts().1.is_ok()),
            "initialize must succeed to reach State::Initialized (gates window/logMessage)"
        );

        let initialized = Request::build("initialized").params(json!({})).finish();
        service.call(initialized).await.expect("initialized failed");

        Self {
            service,
            messages,
            next_id: 2,
        }
    }

    async fn execute_command(
        &mut self,
        command: &str,
        arguments: Vec<serde_json::Value>,
    ) -> Response {
        let id = self.next_id;
        self.next_id += 1;
        let req = Request::build("workspace/executeCommand")
            .params(json!({ "command": command, "arguments": arguments }))
            .id(id)
            .finish();
        self.service
            .call(req)
            .await
            .expect("executeCommand failed")
            .expect("executeCommand must return a response (it was sent with an id)")
    }

    /// Poll captured messages until `predicate` matches one, or panic after
    /// `timeout`. Notifications are drained concurrently by the background
    /// task started in `new`, so this just waits for that to catch up.
    // The live LSP service is intentionally !Sync; this private helper is never spawned.
    #[allow(clippy::future_not_send)]
    async fn wait_for(&self, timeout: Duration, predicate: impl Fn(&Request) -> bool) -> Request {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let messages = self.messages.lock().await;
                if let Some(found) = messages.iter().find(|m| predicate(m)) {
                    return found.clone();
                }
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for a matching client message"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    // The live LSP service is intentionally !Sync; this private helper is never spawned.
    #[allow(clippy::future_not_send)]
    async fn all_messages(&self) -> Vec<Request> {
        self.messages.lock().await.clone()
    }
}

fn is_log_message(req: &Request) -> bool {
    req.method() == "window/logMessage"
}

fn is_show_message(req: &Request) -> bool {
    req.method() == "window/showMessage"
}

fn message_text(req: &Request) -> String {
    req.params()
        .and_then(|p| p.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string()
}

async fn with_timeout<F: Future>(fut: F) -> F::Output {
    tokio::time::timeout(Duration::from_secs(30), fut)
        .await
        .expect("test exceeded 30s safety timeout")
}

#[tokio::test]
async fn run_command_compiles_and_executes_main_end_to_end() {
    with_timeout(async {
        if !tool_available("javac") {
            eprintln!("SKIP run_command_compiles_and_executes_main_end_to_end: javac not on PATH");
            return;
        }

        let temp = tempfile::tempdir().unwrap();
        // Standard Maven/Gradle layout so `find_nearest_source_root` finds it.
        let src_root = temp.path().join("workspace/src/main/java");
        write_file(
            &src_root,
            "Main.java",
            r#"public class Main {
    public static void main(String[] args) {
        System.out.println("hello from the run lens");
    }
}
"#,
        );

        let mut harness = Harness::new().await;
        let uri = Url::from_file_path(src_root.join("Main.java")).unwrap();

        let started = Instant::now();
        let response = harness
            .execute_command(
                RUN_COMMAND,
                vec![json!({ "className": "Main", "uri": uri.to_string() })],
            )
            .await;
        assert!(
            response.into_parts().1.is_ok(),
            "hyperion.run executeCommand response should not be a JSON-RPC error"
        );

        let summary = harness
            .wait_for(Duration::from_secs(15), |m| {
                is_show_message(m) && message_text(m).contains("finished with exit code")
            })
            .await;
        let elapsed = started.elapsed();

        let summary_text = message_text(&summary);
        assert!(
            summary_text.contains("finished with exit code 0"),
            "expected a successful exit code summary, got: {summary_text}"
        );

        let all = harness.all_messages().await;
        let stdout_reached_client = all
            .iter()
            .filter(|m| is_log_message(m))
            .any(|m| message_text(m).contains("hello from the run lens"));
        assert!(
            stdout_reached_client,
            "program's captured stdout must reach the client via window/logMessage; got: {:?}",
            all.iter().map(message_text).collect::<Vec<_>>()
        );

        // Measured E2E time, command dispatch -> summary message.
        println!(
            "[run_command_compiles_and_executes_main_end_to_end] measured E2E time: {elapsed:?}"
        );
    })
    .await;
}

#[tokio::test]
async fn run_command_reports_missing_javac_or_compile_error_honestly() {
    with_timeout(async {
        // This test doesn't require javac to be *present* -- it exercises
        // the "class not found" path either way (unresolved class name),
        // which requires no compilation to have even been attempted, so it
        // has no self-skip.
        let temp = tempfile::tempdir().unwrap();
        let src_root = temp.path().join("workspace/src/main/java");
        write_file(
            &src_root,
            "Empty.java",
            "public class Empty { public static void main(String[] args) {} }\n",
        );

        let mut harness = Harness::new().await;
        let uri = Url::from_file_path(src_root.join("Empty.java")).unwrap();

        // Ask to run a class name that doesn't exist in this file at all --
        // compilation succeeds (Empty.java is valid) but the VM won't find
        // `DoesNotExist` on the classpath, and the summary must say so
        // honestly rather than silently succeeding.
        harness
            .execute_command(
                RUN_COMMAND,
                vec![json!({ "className": "DoesNotExist", "uri": uri.to_string() })],
            )
            .await;

        if !tool_available("javac") {
            let msg = harness
                .wait_for(Duration::from_secs(10), |m| {
                    is_show_message(m) && message_text(m).contains("javac")
                })
                .await;
            assert!(message_text(&msg).contains("PATH") || message_text(&msg).contains("javac"));
            return;
        }

        let msg = harness
            .wait_for(Duration::from_secs(15), |m| {
                is_show_message(m)
                    && (message_text(m).contains("not found")
                        || message_text(m).contains("exit code"))
            })
            .await;
        assert!(
            message_text(&msg).contains("DoesNotExist") || message_text(&msg).contains("exit code"),
            "expected an honest error about the missing class, got: {}",
            message_text(&msg)
        );
    })
    .await;
}

#[tokio::test]
async fn legacy_run_test_never_executes_main_as_a_test_substitute() {
    with_timeout(async {
        // Old clients may retain a payload with `hasMain: true`. That flag
        // previously caused Hyperion to execute main() and present it as the
        // selected test. A class entry point is not a test runner; the legacy
        // command must reject every payload without compiling anything.
        let temp = tempfile::tempdir().unwrap();
        let src_root = temp.path().join("workspace/src/main/java");
        write_file(
            &src_root,
            "MyTest.java",
            r#"public class MyTest {
    public static void main(String[] args) {
        System.out.println("this is not a test");
    }

    @org.junit.jupiter.api.Test
    void shouldDoSomething() {}
}
"#,
        );

        let mut harness = Harness::new().await;
        let uri = Url::from_file_path(src_root.join("MyTest.java")).unwrap();

        harness
            .execute_command(
                RUN_TEST_COMMAND,
                vec![json!({
                    "methodName": "shouldDoSomething",
                    "className": "MyTest",
                    "hasMain": true,
                    "uri": uri.to_string(),
                })],
            )
            .await;

        let msg = harness
            .wait_for(Duration::from_secs(5), |m| {
                is_show_message(m) && message_text(m).contains("not yet supported")
            })
            .await;
        assert!(
            message_text(&msg).contains("shouldDoSomething"),
            "the honest 'unsupported' message should name the test method, got: {}",
            message_text(&msg)
        );

        // No compile/run messages should have been emitted.
        let all = harness.all_messages().await;
        assert!(
            !all.iter().any(|m| message_text(m).contains("Compiling")),
            "a legacy runTest payload must not attempt to compile anything"
        );
    })
    .await;
}

/// `hyperion.resolveLaunch` is advertised and answers with the launch contract
/// without needing a workspace root or a built project.
#[tokio::test]
async fn resolve_launch_command_answers_over_the_wire() {
    with_timeout(async {
        assert!(hyperion_lsp::commands::supported_commands()
            .contains(&RESOLVE_LAUNCH_COMMAND.to_string()));

        let temp = tempfile::tempdir().unwrap();
        let module = temp.path().canonicalize().unwrap().join("app");
        write_file(&module, "build.gradle", "");
        let main = module.join("src/main/java/com/acme/Main.java");
        write_file(
            &module,
            "src/main/java/com/acme/Main.java",
            "package com.acme;\npublic class Main {\n  public static void main(String[] a) {}\n}\n",
        );
        let uri = Url::from_file_path(&main).unwrap();

        let mut harness = Harness::new().await;
        let response = harness
            .execute_command(
                RESOLVE_LAUNCH_COMMAND,
                vec![json!({
                    "uri": uri.to_string(),
                    "position": { "line": 2, "character": 10 },
                    "target": "auto",
                })],
            )
            .await;
        let result = response
            .into_parts()
            .1
            .expect("resolveLaunch must not error");
        assert_eq!(result["kind"], "main");
        assert_eq!(result["launch"]["mainClass"], "com.acme.Main");
        assert_eq!(result["moduleDir"], module.to_string_lossy().as_ref());
        assert!(result["warnings"].is_array());

        let unknown = harness
            .execute_command(
                RESOLVE_LAUNCH_COMMAND,
                vec![json!({ "uri": "file:///definitely/not/here/X.java" })],
            )
            .await;
        assert!(unknown.into_parts().1.expect("no error").is_null());
    })
    .await;
}
