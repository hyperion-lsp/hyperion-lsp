// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Progress reporting for LSP operations
//!
//! Provides registered $/progress notifications for long-running operations
//! like workspace indexing, JDK indexing, and build system resolution.
//!
//! Gracefully degrades if client doesn't support progress notifications.

use lsp_types::notification::Progress;
use lsp_types::request::WorkDoneProgressCreate;
use lsp_types::{
    ProgressParams, ProgressParamsValue, ProgressToken, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd, WorkDoneProgressReport,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tower_lsp::Client;
use tracing::{debug, info, warn};

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProgressState {
    New,
    Active,
    Disabled,
    Ended,
}

/// Helper for reporting progress on long-running LSP operations
///
/// Automatically handles:
/// - Client capability detection (graceful degradation)
/// - Progress token management
/// - Begin/Report/End lifecycle
///
/// Example:
/// ```no_run
/// use hyperion_lsp::progress::ProgressReporter;
/// use tower_lsp::Client;
///
/// async fn report_indexing_progress(client: Client) {
///     let progress = ProgressReporter::new(client, "Indexing Workspace", true);
///     progress.begin(Some("Scanning files...".to_string()), Some(0)).await;
///     progress.report(Some("50/100 files".to_string()), Some(50)).await;
///     progress.end(Some("100 files indexed".to_string())).await;
/// }
/// ```
#[derive(Clone)]
pub struct ProgressReporter {
    client: Client,
    token: ProgressToken,
    title: String,
    supports_progress: bool,
    state: Arc<Mutex<ProgressState>>,
    started: std::time::Instant,
}

impl ProgressReporter {
    /// Create a new progress reporter
    ///
    /// # Arguments
    /// - `client`: LSP client for sending notifications
    /// - `title`: Human-readable title (e.g., "Indexing Workspace")
    /// - `supports_progress`: Whether client supports work_done_progress
    pub fn new(client: Client, title: impl Into<String>, supports_progress: bool) -> Self {
        let title = title.into();
        let token = ProgressToken::String(format!(
            "hyperion/{}",
            NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
        ));

        Self {
            client,
            token,
            title,
            supports_progress,
            state: Arc::new(Mutex::new(ProgressState::New)),
            started: std::time::Instant::now(),
        }
    }

    /// Begin progress reporting
    ///
    /// Shows the initial progress notification in the editor.
    ///
    /// # Arguments
    /// - `message`: Optional detailed message (e.g., "Scanning files...")
    /// - `percentage`: Optional completion percentage (0-100)
    pub async fn begin(&self, message: Option<String>, percentage: Option<u32>) {
        info!(
            title = %self.title,
            status = message.as_deref().unwrap_or_default(),
            elapsed_ms = self.started.elapsed().as_millis(),
            "Progress begin"
        );
        if !self.supports_progress {
            debug!(
                "Progress not supported by client, skipping begin: {}",
                self.title
            );
            return;
        }

        // Clones share this lifecycle: no report can overtake registration or begin.
        let mut state = self.state.lock().await;
        if *state != ProgressState::New {
            return;
        }
        // A failed, timed-out or cancelled registration must never emit progress.
        *state = ProgressState::Disabled;
        let registration =
            self.client
                .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                    token: self.token.clone(),
                });
        match tokio::time::timeout(Duration::from_secs(5), registration).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                warn!(title = %self.title, %error, "Client rejected progress registration");
                return;
            }
            Err(_) => {
                warn!(title = %self.title, "Progress registration timed out; continuing without progress");
                return;
            }
        }

        let begin = WorkDoneProgressBegin {
            title: self.title.clone(),
            cancellable: Some(false),
            message,
            percentage,
        };

        self.client
            .send_notification::<Progress>(ProgressParams {
                token: self.token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(begin)),
            })
            .await;
        *state = ProgressState::Active;

        debug!("Progress begin: {}", self.title);
    }

    /// Report progress update
    ///
    /// Updates the progress notification with new status.
    ///
    /// # Arguments
    /// - `message`: Optional detailed message (e.g., "50/100 files")
    /// - `percentage`: Optional completion percentage (0-100)
    pub async fn report(&self, message: Option<String>, percentage: Option<u32>) {
        info!(
            title = %self.title,
            status = message.as_deref().unwrap_or_default(),
            elapsed_ms = self.started.elapsed().as_millis(),
            "Progress report"
        );
        let state = self.state.lock().await;
        if *state != ProgressState::Active {
            return;
        }

        let report = WorkDoneProgressReport {
            cancellable: Some(false),
            message,
            percentage,
        };

        self.client
            .send_notification::<Progress>(ProgressParams {
                token: self.token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(report)),
            })
            .await;
    }

    /// End progress reporting
    ///
    /// Closes the progress notification.
    ///
    /// # Arguments
    /// - `message`: Optional final message (e.g., "✓ 100 files indexed")
    pub async fn end(&self, message: Option<String>) {
        info!(
            title = %self.title,
            status = message.as_deref().unwrap_or_default(),
            elapsed_ms = self.started.elapsed().as_millis(),
            "Progress end"
        );
        let mut state = self.state.lock().await;
        if *state != ProgressState::Active {
            debug!(
                "Progress not supported by client, skipping end: {}",
                self.title
            );
            return;
        }

        let end = WorkDoneProgressEnd { message };

        self.client
            .send_notification::<Progress>(ProgressParams {
                token: self.token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(end)),
            })
            .await;
        *state = ProgressState::Ended;

        debug!("Progress end: {}", self.title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{FutureExt, SinkExt, StreamExt};
    use serde_json::{json, Value};
    use tower::Service;
    use tower_lsp::jsonrpc::{Error, Request, Response};
    use tower_lsp::lsp_types::{InitializeParams, InitializeResult};
    use tower_lsp::{ClientSocket, LanguageServer, LspService};

    struct TestServer(Client);

    #[tower_lsp::async_trait]
    impl LanguageServer for TestServer {
        async fn initialize(
            &self,
            _: InitializeParams,
        ) -> tower_lsp::jsonrpc::Result<InitializeResult> {
            Ok(InitializeResult::default())
        }

        async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
            Ok(())
        }
    }

    async fn harness(
        supports_progress: bool,
    ) -> (LspService<TestServer>, ClientSocket, ProgressReporter) {
        let (mut service, socket) = LspService::new(TestServer);
        let response = service
            .call(
                Request::build("initialize")
                    .params(json!({"capabilities": {}}))
                    .id(1)
                    .finish(),
            )
            .await
            .expect("initialize dispatch")
            .expect("initialize response");
        assert!(response.is_ok());
        let progress = ProgressReporter::new(
            service.inner().0.clone(),
            "Indexing Workspace",
            supports_progress,
        );
        (service, socket, progress)
    }

    fn assert_no_message(socket: &mut ClientSocket) {
        assert!(
            socket.next().now_or_never().is_none(),
            "unexpected progress message"
        );
    }

    async fn receive(socket: &mut ClientSocket) -> Request {
        tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("progress message deadline")
            .expect("progress message")
    }

    #[tokio::test]
    async fn progress_waits_for_registration_and_shares_lifecycle_across_clones() {
        let (_service, mut socket, progress) = harness(true).await;
        progress.report(None, Some(1)).await;
        progress.end(None).await;
        assert_no_message(&mut socket);

        let cloned = progress.clone();
        let begin = tokio::spawn(async move {
            cloned.begin(Some("Scanning".into()), Some(0)).await;
        });
        let create = receive(&mut socket).await;
        assert_eq!(create.method(), "window/workDoneProgress/create");
        let token = create.params().expect("create params")["token"].clone();
        assert_no_message(&mut socket);
        socket
            .send(Response::from_ok(
                create.id().expect("request id").clone(),
                Value::Null,
            ))
            .await
            .expect("approve token");
        begin.await.expect("begin task");

        let begin = receive(&mut socket).await;
        assert_eq!(begin.method(), "$/progress");
        assert_eq!(begin.params().unwrap()["token"], token);
        assert_eq!(
            begin.params().unwrap()["value"],
            json!({"kind": "begin", "title": "Indexing Workspace", "message": "Scanning", "percentage": 0, "cancellable": false})
        );
        let cloned = progress.clone();
        let updates = tokio::spawn(async move {
            cloned.begin(None, None).await;
            cloned.report(Some("Halfway".into()), Some(50)).await;
            cloned.end(Some("Done".into())).await;
        });
        for kind in ["report", "end"] {
            let notification = receive(&mut socket).await;
            assert_eq!(notification.method(), "$/progress");
            assert_eq!(notification.params().unwrap()["token"], token);
            assert_eq!(notification.params().unwrap()["value"]["kind"], kind);
        }
        updates.await.expect("progress updates");
        progress.report(None, None).await;
        progress.end(None).await;
        progress.begin(None, None).await;
        assert_no_message(&mut socket);

        let other = ProgressReporter::new(progress.client.clone(), "Indexing Workspace", true);
        assert_ne!(
            other.token, progress.token,
            "repeated titles need distinct tokens"
        );
    }

    #[tokio::test]
    async fn progress_is_silent_without_client_support() {
        let (_service, mut socket, progress) = harness(false).await;
        progress.begin(None, None).await;
        progress.report(None, None).await;
        progress.end(None).await;
        assert_no_message(&mut socket);
    }

    #[tokio::test]
    async fn progress_is_silent_after_registration_rejection() {
        let (_service, mut socket, progress) = harness(true).await;
        let cloned = progress.clone();
        let begin = tokio::spawn(async move {
            cloned.begin(None, None).await;
        });
        let create = receive(&mut socket).await;
        socket
            .send(Response::from_error(
                create.id().unwrap().clone(),
                Error::method_not_found(),
            ))
            .await
            .expect("reject token");
        begin.await.expect("begin task");
        progress.report(None, None).await;
        progress.end(None).await;
        progress.begin(None, None).await;
        assert_no_message(&mut socket);
    }

    #[tokio::test(start_paused = true)]
    async fn progress_registration_timeout_does_not_block_work() {
        let (_service, mut socket, progress) = harness(true).await;
        let cloned = progress.clone();
        let begin = tokio::spawn(async move {
            cloned.begin(None, None).await;
        });
        let create = receive(&mut socket).await;
        tokio::time::advance(Duration::from_secs(6)).await;
        begin.await.expect("registration timed out");
        socket
            .send(Response::from_ok(create.id().unwrap().clone(), Value::Null))
            .await
            .expect("late registration response must not panic");
        progress.report(None, None).await;
        progress.end(None).await;
        assert_no_message(&mut socket);
    }
}
