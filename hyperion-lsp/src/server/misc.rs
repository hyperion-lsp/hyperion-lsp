// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;
use std::time::Duration;

use tower_lsp::lsp_types::{
    CodeLens, CodeLensParams, DocumentLink, DocumentLinkParams, ExecuteCommandParams, FoldingRange,
    FoldingRangeParams, MessageType, SemanticTokens, SemanticTokensParams, SemanticTokensResult,
    Url,
};
use tracing::{debug, warn};

use crate::commands::{parse_command, CommandAction, RUN_COMMAND};
use crate::run_config::discover_run_configs;
use crate::run_executor::{self, DEFAULT_TIMEOUT_MS};
use crate::server::Backend;

impl Backend {
    pub(crate) async fn handle_semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Option<SemanticTokensResult> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        let uri = params.text_document.uri;
        debug!("Semantic tokens request for {}", uri);

        let (file_id, source_text) = if let Some(state) = self.documents.by_uri.get(uri.as_str()) {
            (state.file_id, state.text.clone())
        } else {
            warn!("Semantic tokens requested for unopened document: {}", uri);
            return None;
        };

        match self
            .services
            .semantic_tokens
            .semantic_tokens(&txn, file_id, &source_text)
        {
            Ok(tokens) => {
                debug!("Generated {} semantic tokens", tokens.len());
                Some(SemanticTokensResult::Tokens(SemanticTokens {
                    result_id: None,
                    data: tokens,
                }))
            }
            Err(e) => {
                warn!("Semantic tokens error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> Option<Vec<FoldingRange>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        let uri = params.text_document.uri;
        debug!("Folding range request for {}", uri);

        let (file_id, source_text) = self.java_document(&uri, "Folding ranges")?;

        match self
            .services
            .folding
            .folding_ranges(&txn, file_id, &source_text)
        {
            Ok(ranges) => {
                debug!("Generated {} folding ranges", ranges.len());
                Some(ranges)
            }
            Err(e) => {
                warn!("Folding range error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_document_link(
        &self,
        params: DocumentLinkParams,
    ) -> Option<Vec<DocumentLink>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        let uri = params.text_document.uri;
        debug!("Document link request for {}", uri);

        let (file_id, source_text) = self.java_document(&uri, "Document links")?;

        match self
            .services
            .document_links
            .document_links(&txn, file_id, &source_text)
        {
            Ok(links) => {
                debug!("Generated {} document links", links.len());
                Some(links)
            }
            Err(e) => {
                warn!("Document link error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_code_lens(&self, params: CodeLensParams) -> Option<Vec<CodeLens>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        let uri = params.text_document.uri;
        debug!("Code lens request for {}", uri);

        let (file_id, source_text) = self.java_document(&uri, "Code lenses")?;

        match self
            .services
            .code_lens
            .code_lens(&txn, file_id, &source_text)
        {
            Ok(mut lenses) => {
                debug!("Generated {} code lenses", lenses.len());
                // `CodeLensProvider` only knows about the parsed AST, not the
                // originating document. `hyperion.run` needs the source URI
                // when it comes back through `workspace/executeCommand`, so
                // add it here rather than plumbing it through the provider.
                for lens in &mut lenses {
                    if let Some(command) = &mut lens.command {
                        if command.command != RUN_COMMAND {
                            continue;
                        }
                        if let Some(first_arg) = command
                            .arguments
                            .get_or_insert_with(Vec::new)
                            .first_mut()
                            .and_then(|v| v.as_object_mut())
                        {
                            first_arg.insert(
                                "uri".to_string(),
                                serde_json::Value::String(uri.to_string()),
                            );
                        }
                    }
                }
                Some(lenses)
            }
            Err(e) => {
                warn!("Code lens error: {}", e);
                None
            }
        }
    }

    /// Re-read the Gradle/Maven model now (debounced with any pending refresh).
    pub(crate) fn reload_project(&self, reason: &str) {
        let Some(root) = self.workspace.root.read().clone() else {
            warn!("Project reload requested but no workspace root is set");
            return;
        };
        crate::server::schedule_build_system_refresh(
            self.background_tasks.build_refresh.clone(),
            root,
            self.client.clone(),
            self.invalidation_runtime(),
            Arc::clone(&self.services.source_providers),
            reason,
        );
    }

    pub(crate) async fn handle_execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Option<serde_json::Value> {
        let command = params.command.clone();
        let action = parse_command(&command, Some(&params.arguments));

        match action {
            Some(CommandAction::Run { class_name, uri }) => {
                self.run_main_class(class_name, uri).await;
            }
            Some(CommandAction::RunTest { method_name }) => {
                // Backward compatibility for clients that cached an old
                // lens. A class `main` is not a test runner and must never be
                // executed as a substitute for the selected test.
                let name = method_name.unwrap_or_else(|| "<unknown>".to_string());
                self.client
                    .show_message(
                        MessageType::WARNING,
                        format!(
                            "Cannot run test '{name}': test framework execution is not yet \
                             supported."
                        ),
                    )
                    .await;
            }
            Some(CommandAction::ShowImplementations { class_name }) => {
                let name = class_name.unwrap_or_else(|| "<unknown>".to_string());
                self.client
                    .show_message(
                        MessageType::WARNING,
                        format!(
                            "Cannot show implementations for '{name}': this legacy command is no \
                             longer advertised because the LSP has no portable show-locations \
                             command. Use Go to Implementation instead."
                        ),
                    )
                    .await;
            }
            Some(CommandAction::ReloadProject) => {
                // An explicit reload re-runs the build tool even when no build
                // file changed (a dependency may have been published since).
                crate::build_tool_classpath::request_rerun();
                self.reload_project("hyperion.reloadProject");
            }
            Some(CommandAction::ResolveLaunch {
                uri,
                position,
                target,
            }) => {
                return Some(
                    self.resolve_launch_command(uri, position, target.as_deref())
                        .await,
                );
            }
            Some(CommandAction::RunConfigurations) => {
                let root = self.workspace.root.read().clone();
                if let Some(root) = root {
                    let store = self.workspace.project_classpaths.clone();
                    // Reads run-configuration files and walks the project to
                    // find each main class's module: keep it off the async loop.
                    let configs = match tokio::task::spawn_blocking(move || {
                        let mut configs = discover_run_configs(&root);
                        for config in &mut configs {
                            crate::launch::enrich_application_config(config, &root, &store);
                        }
                        configs
                    })
                    .await
                    {
                        Ok(configs) => configs,
                        Err(error) => {
                            // A panic in the blocking task must not look like "no run configurations".
                            tracing::error!("run configuration discovery failed: {error}");
                            Vec::new()
                        }
                    };
                    debug!("Discovered {} run configurations", configs.len());
                    return Some(serde_json::Value::Array(configs));
                }
                warn!("RunConfigurations requested but no workspace root set");
            }
            Some(CommandAction::SafeDelete {
                uri,
                line,
                character,
                version,
            }) => {
                return self
                    .execute_safe_delete_command(uri, line, character, version)
                    .await;
            }
            None => {
                self.log_message(
                    MessageType::WARNING,
                    format!("Unknown executeCommand request: '{command}'"),
                )
                .await;
            }
        }

        None
    }

    /// `hyperion.resolveLaunch`: never fails the request; a file that has
    /// nothing to launch yields `null`.
    async fn resolve_launch_command(
        &self,
        uri: Option<String>,
        position: Option<(u32, u32)>,
        target: Option<&str>,
    ) -> serde_json::Value {
        let Some(path) = uri
            .as_deref()
            .and_then(|uri| Url::parse(uri).ok())
            .and_then(|uri| uri.to_file_path().ok())
        else {
            return serde_json::Value::Null;
        };
        let text = if let Some(state) = uri
            .as_deref()
            .and_then(|uri| self.documents.by_uri.get(uri))
        {
            (*state.text).clone()
        } else if let Ok(text) = hyperion_workspace::read_source_file(&path) {
            text
        } else {
            return serde_json::Value::Null;
        };
        let workspace_root = self
            .workspace
            .root
            .read()
            .clone()
            .or_else(|| path.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_default();
        let store = self.workspace.project_classpaths.clone();
        let target = crate::launch::LaunchTarget::parse(target);
        let position =
            position.map(|(line, character)| tower_lsp::lsp_types::Position { line, character });
        tokio::task::spawn_blocking(move || {
            crate::launch::resolve_launch(&crate::launch::LaunchRequest {
                path: &path,
                text: &text,
                position,
                target,
                workspace_root: &workspace_root,
                store: &store,
            })
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(serde_json::Value::Null)
    }

    /// Compiles `class_name`'s source (found via `uri`, the file the "▶ Run"
    /// lens was invoked from) with `javac` and executes its
    /// `main` in the in-process VM, streaming output to the client as
    /// `window/logMessage` lines plus a final `window/showMessage` summary.
    ///
    /// Runs `run_executor::compile_and_run` inside `spawn_blocking` with a
    /// hard wall-clock timeout: the VM has no cancellation hook,
    /// so on timeout the blocking-pool thread (and whatever it's doing --
    /// javac, or an infinite loop inside the VM) is simply abandoned rather
    /// than stopped. That only unblocks the LSP's async event loop; it does
    /// not reclaim the thread or the VM's in-progress work.
    async fn run_main_class(&self, class_name: Option<String>, uri: Option<String>) {
        let Some(class_name) = class_name else {
            self.client
                .show_message(MessageType::ERROR, "Cannot run: no class name provided.")
                .await;
            return;
        };
        let Some(uri) = uri else {
            self.client
                .show_message(
                    MessageType::ERROR,
                    "Cannot run: no source file associated with this command.",
                )
                .await;
            return;
        };
        let Ok(parsed_uri) = Url::parse(&uri) else {
            self.client
                .show_message(
                    MessageType::ERROR,
                    format!("Cannot run: invalid URI '{uri}'"),
                )
                .await;
            return;
        };
        let Ok(file_path) = parsed_uri.to_file_path() else {
            self.client
                .show_message(
                    MessageType::ERROR,
                    format!("Cannot run: URI is not a local file: '{uri}'"),
                )
                .await;
            return;
        };

        // Prefer the client's in-memory buffer (unsaved edits) over disk.
        let source_text: String = if let Some(state) = self.documents.by_uri.get(uri.as_str()) {
            (*state.text).clone()
        } else {
            match hyperion_workspace::read_source_file(&file_path) {
                Ok(text) => text,
                Err(e) => {
                    self.client
                        .show_message(
                            MessageType::ERROR,
                            format!("Cannot run: failed to read {}: {e}", file_path.display()),
                        )
                        .await;
                    return;
                }
            }
        };

        let workspace_root = self.workspace.root.read().clone();

        self.client
            .log_message(MessageType::INFO, format!("Running '{class_name}'..."))
            .await;

        let timeout = Duration::from_millis(DEFAULT_TIMEOUT_MS);
        let blocking_class_name = class_name.clone();
        let project_classpaths = self.workspace.project_classpaths.clone();
        let handle = tokio::task::spawn_blocking(move || {
            run_executor::compile_and_run_with_project_classpaths(
                &file_path,
                &source_text,
                &blocking_class_name,
                workspace_root.as_deref(),
                &project_classpaths,
            )
        });

        let outcome = match tokio::time::timeout(timeout, handle).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(join_err)) => {
                self.client
                    .show_message(
                        MessageType::ERROR,
                        format!("Run '{class_name}' panicked: {join_err}"),
                    )
                    .await;
                return;
            }
            Err(_) => {
                self.client
                    .show_message(
                        MessageType::ERROR,
                        format!(
                            "Run '{class_name}' timed out after {DEFAULT_TIMEOUT_MS}ms and was \
                             abandoned (the VM has no cancellation hook, so the underlying \
                             thread keeps running in the background)."
                        ),
                    )
                    .await;
                return;
            }
        };

        for (typ, line) in outcome.log_lines {
            self.client.log_message(typ, line).await;
        }
        self.client
            .show_message(outcome.summary_type, outcome.summary)
            .await;
    }
}
