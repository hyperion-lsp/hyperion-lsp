// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse, Command,
    DocumentChangeOperation, DocumentChanges, MessageType, OneOf,
    OptionalVersionedTextDocumentIdentifier, Position, PrepareRenameResponse, RenameFile,
    RenameFileOptions, RenameParams, ResourceOp, TextDocumentEdit, TextDocumentPositionParams, Url,
    WorkspaceEdit,
};
use tower_lsp::CancellationToken;
use tracing::{debug, warn};

use crate::commands::SAFE_DELETE_COMMAND;
use crate::server::Backend;

impl Backend {
    fn destructive_refactoring_generation_is_current(&self) -> bool {
        self.workspace.destructive_refactoring.is_ready()
            && self.workspace.root.read().as_deref().is_none_or(|root| {
                self.workspace
                    .project_classpaths
                    .published_archive_generation_is_current(root)
            })
    }

    pub(crate) fn handle_prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Option<PrepareRenameResponse> {
        let uri = params.text_document.uri;
        let position = params.position;

        debug!("Prepare rename request for {} at {:?}", uri, position);

        let (file_id, source_text) = if let Some(state) = self
            .documents
            .by_uri
            .get(uri.as_str())
            .filter(|state| !state.desynchronized)
        {
            (state.file_id, state.text.clone())
        } else {
            warn!("Prepare rename requested for unopened document: {}", uri);
            return None;
        };

        let workspace_root = self.workspace.root.read().clone();
        if let (Some(root), Ok(path), Ok(offset)) = (
            workspace_root,
            uri.to_file_path(),
            crate::position_utils::position_to_offset(&source_text, position),
        ) {
            if let Some(package) = crate::file_move::package_at(
                &self.workspace.index,
                &root,
                &path,
                &source_text,
                offset,
            ) {
                return Some(PrepareRenameResponse::Range(
                    tower_lsp::lsp_types::Range::new(
                        crate::position_utils::offset_to_position(&source_text, package.start),
                        crate::position_utils::offset_to_position(&source_text, package.end),
                    ),
                ));
            }
        }

        match self
            .services
            .rename
            .prepare_rename(file_id, position, &source_text, &uri)
        {
            Ok(Some(range)) => {
                debug!("Symbol can be renamed at range: {:?}", range);
                Some(PrepareRenameResponse::Range(range))
            }
            Ok(None) => {
                debug!("Symbol at position cannot be renamed");
                None
            }
            Err(e) => {
                warn!("Prepare rename error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_rename(
        &self,
        params: RenameParams,
    ) -> Result<Option<WorkspaceEdit>> {
        if self.has_desynchronized_open_document() {
            return Ok(None);
        }
        let cancellation = CancellationToken::current();
        cancellation.check()?;
        tokio::task::yield_now().await;
        cancellation.check()?;

        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        debug!(
            "Rename request for {} at {:?} to '{}'",
            uri, position, new_name
        );

        let Some((file_id, source_text, request_version)) = self
            .documents
            .by_uri
            .get(uri.as_str())
            .filter(|state| !state.desynchronized)
            .map(|state| (state.file_id, state.text.clone(), state.version))
        else {
            warn!("Rename requested for unopened document: {}", uri);
            return Ok(None);
        };

        let source_root_ready = match uri.to_file_path() {
            Ok(file_path) => {
                let ready = self
                    .ensure_open_file_source_root_indexed(&file_path, true)
                    .await;
                cancellation.check()?;
                ready
            }
            Err(()) => false,
        };
        if let Some(edit) = self
            .try_rename_package(&uri, position, &source_text, &new_name, source_root_ready)
            .await?
        {
            return Ok(Some(edit));
        }
        let selected_still_current = self
            .documents
            .by_uri
            .get(uri.as_str())
            .is_some_and(|state| {
                state.version == request_version && state.text.as_str() == source_text.as_str()
            });
        if !selected_still_current {
            return Ok(None);
        }

        let open_versions_before = open_document_versions(&self.documents.by_uri);
        let supports_versioned_edits = self.supports_versioned_workspace_edits();
        // Provider completeness describes semantic readiness only. Protocol
        // support is checked after the proof reveals whether this is local,
        // workspace-wide, or also requires a resource operation.
        let workspace_complete =
            source_root_ready && self.destructive_refactoring_generation_is_current();
        let Ok(permit) = self.workspace.host.read_interactive().await else {
            return Ok(None);
        };
        cancellation.check()?;

        let provider = self.services.rename.clone();
        let blocking_cancellation = cancellation.clone();
        let blocking_source = Arc::clone(&source_text);
        let blocking_uri = uri.clone();
        let result = match tokio::task::spawn_blocking(move || {
            provider.rename_proof_cancellable_with_workspace_completeness(
                file_id,
                position,
                &new_name,
                &blocking_source,
                &blocking_uri,
                workspace_complete,
                &blocking_cancellation,
            )
        })
        .await
        {
            Ok(result) => result,
            Err(error) => {
                warn!("Rename blocking task failed: {error}");
                return Err(Error::internal_error());
            }
        };
        drop(permit);
        cancellation.check()?;
        let open_versions_after = open_document_versions(&self.documents.by_uri);
        let selected_still_current = self
            .documents
            .by_uri
            .get(uri.as_str())
            .is_some_and(|state| {
                state.version == request_version && state.text.as_str() == source_text.as_str()
            });
        if self.has_desynchronized_open_document()
            || !selected_still_current
            || open_versions_after != open_versions_before
            || (workspace_complete && !self.destructive_refactoring_generation_is_current())
        {
            return Ok(None);
        }
        match result {
            Ok(proof) => {
                debug!("Rename completed successfully");
                let Some(proof) = proof else {
                    return Ok(None);
                };
                if proof.requires_versioned_workspace_edit() && !supports_versioned_edits {
                    return Err(Error::invalid_params(
                        "Workspace rename requires client support for versioned documentChanges",
                    ));
                }
                let supports_file_rename = self.supports_rename_file_workspace_edits();
                if proof.file_rename.is_some() && !supports_file_rename {
                    return Err(Error::invalid_params(
                        "Renaming this public top-level Java type requires client support for documentChanges and the Rename resource operation",
                    ));
                }
                if proof.file_rename.as_ref().is_some_and(|plan| {
                    plan.new_uri != plan.old_uri && open_versions_before.contains_key(&plan.new_uri)
                }) {
                    return Err(Error::invalid_params(
                        "Cannot rename the Java source file because the target URI is already open",
                    ));
                }
                let requires_workspace_readiness = proof.requires_versioned_workspace_edit();

                let validation_index = Arc::clone(&self.workspace.index);
                let validation_cancellation = cancellation.clone();
                let proof = match tokio::task::spawn_blocking(move || {
                    proof
                        .is_current_cancellable(&validation_index, &|| {
                            validation_cancellation.is_cancelled()
                        })
                        .then_some(proof)
                })
                .await
                {
                    Ok(Some(proof)) => proof,
                    Ok(None) => return Ok(None),
                    Err(error) => {
                        warn!("Rename proof validation task failed: {error}");
                        return Err(Error::internal_error());
                    }
                };

                // Validation performs filesystem I/O off-thread. Recheck all
                // async-owned state after awaiting it, not before.
                let open_versions_after_validation = open_document_versions(&self.documents.by_uri);
                let selected_still_current =
                    self.documents
                        .by_uri
                        .get(uri.as_str())
                        .is_some_and(|state| {
                            state.version == request_version
                                && state.text.as_str() == source_text.as_str()
                        });
                if cancellation.is_cancelled()
                    || self.has_desynchronized_open_document()
                    || !selected_still_current
                    || open_versions_after_validation != open_versions_before
                    || (requires_workspace_readiness
                        && !self.destructive_refactoring_generation_is_current())
                {
                    return Ok(None);
                }

                // Closed-file edits have no LSP compare-and-swap primitive;
                // a watcher mutation after this final proof check remains the
                // protocol's residual race.
                let edit = prepare_rename_workspace_edit_for_client(
                    proof.edit,
                    &open_versions_before,
                    supports_versioned_edits,
                    supports_file_rename,
                    proof.file_rename,
                )
                .map_err(Error::invalid_params)?;
                Ok(Some(edit))
            }
            Err(e) => {
                warn!("Rename error: {}", e);
                Err(Error::invalid_params(e))
            }
        }
    }

    /// Rename of a package segment: moves the package's directories and
    /// rewrites declarations and imports. `Ok(None)` when the cursor is not on
    /// a package name (the ordinary symbol rename then runs).
    async fn try_rename_package(
        &self,
        uri: &Url,
        position: Position,
        source_text: &Arc<String>,
        new_name: &str,
        source_root_ready: bool,
    ) -> Result<Option<WorkspaceEdit>> {
        let (Some(root), Ok(path), Ok(offset)) = (
            self.workspace.root.read().clone(),
            uri.to_file_path(),
            crate::position_utils::position_to_offset(source_text, position),
        ) else {
            return Ok(None);
        };
        let Some(target) =
            crate::file_move::package_at(&self.workspace.index, &root, &path, source_text, offset)
        else {
            return Ok(None);
        };
        if !self.supports_rename_file_workspace_edits() {
            return Err(Error::invalid_params(
                "Renaming a package requires client support for documentChanges and resource operations",
            ));
        }
        if !source_root_ready || !self.destructive_refactoring_generation_is_current() {
            return Err(Error::invalid_params(
                "Cannot rename the package: workspace indexing is incomplete",
            ));
        }
        let Ok(permit) = self.workspace.host.read_interactive().await else {
            return Ok(None);
        };
        let index = Arc::clone(&self.workspace.index);
        let new_name = new_name.to_string();
        let plan = tokio::task::spawn_blocking(move || {
            crate::file_move::plan_package_rename(&index, &root, &target, &new_name)
        })
        .await
        .map_err(|_| Error::internal_error())?
        .map_err(Error::invalid_params)?;
        drop(permit);
        let versions = open_document_versions(&self.documents.by_uri);
        crate::file_move::plan_to_workspace_edit(
            &plan,
            crate::file_move::EditStyle::ResourceOperations,
            &versions,
        )
        .map(Some)
        .ok_or_else(|| Error::invalid_params("Package rename could not be encoded"))
    }

    pub(crate) async fn handle_code_action(
        &self,
        params: CodeActionParams,
    ) -> Option<CodeActionResponse> {
        if self.has_desynchronized_open_document() {
            return None;
        }
        let cancellation = CancellationToken::current();
        if cancellation.is_cancelled() {
            return None;
        }
        // Cancellation checkpoint: Allow tower-lsp to abort this request if $/cancelRequest was sent
        tokio::task::yield_now().await;
        if cancellation.is_cancelled() {
            return None;
        }

        let uri = params.text_document.uri.clone();

        debug!(
            "Code action request for {} at range {:?}",
            uri, params.range
        );

        let (file_id, source_text) = self.java_document(&uri, "Code actions")?;
        let request_version = self.documents.by_uri.get(uri.as_str())?.version;
        let open_versions: std::collections::HashMap<Url, i32> = self
            .documents
            .by_uri
            .iter()
            .filter_map(|state| Url::parse(&state.uri).ok().map(|uri| (uri, state.version)))
            .collect();

        let workspace_was_ready = self
            .workspace
            .ready
            .load(std::sync::atomic::Ordering::Acquire);
        let source_root_ready = if workspace_was_ready {
            if let Ok(file_path) = uri.to_file_path() {
                self.ensure_open_file_source_root_indexed(&file_path, true)
                    .await
            } else {
                false
            }
        } else {
            false
        };
        if workspace_was_ready && cancellation.is_cancelled() {
            return None;
        }

        // Enter the read transaction only now: the source-root indexing awaited
        // above takes the write transaction, so waiting for it while holding a
        // read transaction could only ever run into that wait's timeout.
        let txn = self.workspace.host.read_interactive().await.ok()?;

        // Refactorings may parse and perform an exact workspace-wide usage
        // scan. Keep that CPU and I/O off Tokio's async worker, and thread the
        // request cancellation token into the long scan.
        let index = Arc::clone(&self.workspace.index);
        let blocking_source = source_text.clone();
        let blocking_params = params.clone();
        let blocking_cancellation = cancellation.clone();
        let safe_delete_available = self.supports_safe_delete_workspace_edits()
            && self.destructive_refactoring_generation_is_current()
            && source_root_ready;
        let (mut all_actions, safe_delete_candidate) = match txn
            .run_blocking(move |txn| {
                let provider = crate::code_actions::CodeActionProvider::new(txn, index);
                let candidate = safe_delete_available.then(|| {
                    provider.safe_delete_candidate_name(
                        file_id,
                        &blocking_params.text_document.uri,
                        &blocking_source,
                        blocking_params.range.start,
                    )
                });
                let actions = provider.code_actions_cancellable(
                    file_id,
                    &blocking_params,
                    &blocking_source,
                    false,
                    || blocking_cancellation.is_cancelled(),
                );
                (actions, candidate.flatten())
            })
            .await
            .ok()?
        {
            (Ok(Some(actions)), candidate) => (actions, candidate),
            (Ok(None), candidate) => (Vec::new(), candidate),
            (Err(e), _) => {
                warn!("Code action error: {}", e);
                return None;
            }
        };
        if cancellation.is_cancelled() {
            return None;
        }

        // Do not return a formerly-correct range for a newer open document.
        let current = self.documents.by_uri.get(uri.as_str())?;
        if current.version != request_version || current.text.as_str() != source_text.as_str() {
            return None;
        }
        drop(current);
        if self.has_desynchronized_open_document() {
            return None;
        }

        if let Some(symbol_name) = safe_delete_candidate {
            all_actions.push(CodeActionOrCommand::CodeAction(safe_delete_command_action(
                &symbol_name,
                &uri,
                params.range.start,
                request_version,
            )));
        }

        // Moving a type to another package needs resource operations (the
        // source file itself moves), so it is built here with the client's
        // capabilities rather than inside the capability-agnostic provider.
        if self.supports_rename_file_workspace_edits()
            && source_root_ready
            && self.destructive_refactoring_generation_is_current()
            && !cancellation.is_cancelled()
        {
            let root = self.workspace.root.read().clone();
            if let (Some(root), Ok(path)) = (root, uri.to_file_path()) {
                let index = Arc::clone(&self.workspace.index);
                let source = source_text.clone();
                let offset =
                    crate::position_utils::position_to_offset(&source, params.range.start).ok();
                let versions = open_versions.clone();
                if let Some(offset) = offset {
                    let moves = match tokio::task::spawn_blocking(move || {
                        crate::file_move::move_to_package_actions(
                            &index,
                            &root,
                            &path,
                            &source,
                            offset,
                            crate::file_move::MoveCapabilities {
                                document_changes: true,
                                rename_file: true,
                            },
                            &versions,
                        )
                    })
                    .await
                    {
                        Ok(moves) => moves,
                        Err(error) => {
                            // A panic in the blocking task must not look like "no move actions".
                            tracing::error!("move-to-package action discovery failed: {error}");
                            Vec::new()
                        }
                    };
                    all_actions.extend(moves.into_iter().map(CodeActionOrCommand::CodeAction));
                }
            }
        }
        if cancellation.is_cancelled() {
            return None;
        }

        // With CreateFile support the extracted interface goes into its own
        // file next to the class instead of into the same file.
        if self.supports_create_file_workspace_edits() && !cancellation.is_cancelled() {
            if let Ok(offset) =
                crate::position_utils::position_to_offset(&source_text, params.range.start)
            {
                let index = Arc::clone(&self.workspace.index);
                let source = source_text.clone();
                let target = uri.clone();
                let version = request_version;
                let upgraded = tokio::task::spawn_blocking(move || {
                    let package = index
                        .get_file_id(&target.to_file_path().ok()?)
                        .and_then(|id| index.get_imports(id))
                        .and_then(|imports| imports.current_package)
                        .unwrap_or_default();
                    let plan =
                        crate::code_actions::extract_interface::plan(&source, offset, |name| {
                            let fqn = if package.is_empty() {
                                name.to_string()
                            } else {
                                format!("{package}.{name}")
                            };
                            index.has_class(&fqn)
                        })?;
                    let title = format!("Extract interface '{}'", plan.interface_name);
                    plan.new_file_action(&target, &source, Some(version), |path| path.exists())
                        .map(|action| (title, action))
                })
                .await
                .ok()
                .flatten();
                if let Some((title, action)) = upgraded {
                    all_actions.retain(|existing| {
                        !matches!(existing, CodeActionOrCommand::CodeAction(a) if a.title == title)
                    });
                    all_actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
        }

        // Add security-related code actions
        let security_actions =
            self.services
                .security
                .code_actions_at_position(&uri, params.range.start, &source_text);
        all_actions.extend(security_actions);
        if let Some(only) = params.context.only.as_ref() {
            all_actions.retain(|action| match action {
                CodeActionOrCommand::CodeAction(action) => {
                    action.kind.as_ref().is_some_and(|kind| {
                        only.iter()
                            .any(|requested| code_action_kind_matches(requested, kind))
                    })
                }
                CodeActionOrCommand::Command(_) => false,
            });
        }
        let supports_versioned_edits = self.supports_versioned_workspace_edits();
        for action in &mut all_actions {
            if let CodeActionOrCommand::CodeAction(action) = action {
                if let Some(edit) = action.edit.take() {
                    action.edit = Some(prepare_workspace_edit_for_client(
                        edit,
                        &open_versions,
                        supports_versioned_edits,
                        false,
                    ));
                }
            }
        }

        debug!("Returning {} total code actions", all_actions.len());

        if all_actions.is_empty() {
            None
        } else {
            Some(all_actions)
        }
    }

    fn supports_safe_delete_workspace_edits(&self) -> bool {
        self.client_workspace_edit_capabilities
            .load(std::sync::atomic::Ordering::Acquire)
            & 0b11
            == 0b11
    }

    pub(super) fn supports_versioned_workspace_edits(&self) -> bool {
        self.client_workspace_edit_capabilities
            .load(std::sync::atomic::Ordering::Acquire)
            & 0b10
            != 0
    }

    pub(super) fn supports_create_file_workspace_edits(&self) -> bool {
        let capabilities = self
            .client_workspace_edit_capabilities
            .load(std::sync::atomic::Ordering::Acquire);
        capabilities & 0b10 != 0 && capabilities & 0b1000 != 0
    }

    fn supports_delete_file_workspace_edits(&self) -> bool {
        let capabilities = self
            .client_workspace_edit_capabilities
            .load(std::sync::atomic::Ordering::Acquire);
        capabilities & 0b10 != 0 && capabilities & 0b1_0000 != 0
    }

    fn supports_rename_file_workspace_edits(&self) -> bool {
        let capabilities = self
            .client_workspace_edit_capabilities
            .load(std::sync::atomic::Ordering::Acquire);
        capabilities & 0b10 != 0 && capabilities & 0b100 != 0
    }

    pub(crate) async fn execute_safe_delete_command(
        &self,
        uri: Option<String>,
        line: Option<u32>,
        character: Option<u32>,
        version: Option<i32>,
    ) -> Option<serde_json::Value> {
        let cancelled = || serde_json::json!({ "applied": false });
        if self.has_desynchronized_open_document() {
            return Some(cancelled());
        }
        let cancellation = CancellationToken::current();
        if cancellation.is_cancelled() {
            return Some(cancelled());
        }
        let (Some(uri), Some(line), Some(character), Some(version)) =
            (uri, line, character, version)
        else {
            self.client
                .show_message(
                    MessageType::WARNING,
                    "Safe delete command payload is incomplete",
                )
                .await;
            return Some(serde_json::json!({ "applied": false }));
        };
        if !self.supports_safe_delete_workspace_edits() {
            self.client
                .show_message(
                    MessageType::WARNING,
                    "Safe delete requires client support for workspace/applyEdit and versioned documentChanges",
                )
                .await;
            return Some(cancelled());
        }
        let Ok(uri) = Url::parse(&uri) else {
            self.client
                .show_message(MessageType::WARNING, "Safe delete document URI is invalid")
                .await;
            return Some(serde_json::json!({ "applied": false }));
        };
        let selected = self
            .documents
            .by_uri
            .get(uri.as_str())
            .filter(|state| !state.desynchronized)
            .map(|state| (state.file_id, state.text.clone(), state.version));
        let (file_id, source_text, current_version) = match selected {
            Some(selected) => selected,
            None => {
                self.client
                    .show_message(
                        MessageType::WARNING,
                        "Safe delete document is no longer open",
                    )
                    .await;
                return Some(cancelled());
            }
        };
        if current_version != version {
            self.client
                .show_message(
                    MessageType::WARNING,
                    "Safe delete was not applied because the document changed",
                )
                .await;
            return Some(cancelled());
        }
        if !self
            .workspace
            .ready
            .load(std::sync::atomic::Ordering::Acquire)
            || !self.destructive_refactoring_generation_is_current()
        {
            return Some(cancelled());
        }
        let Ok(file_path) = uri.to_file_path() else {
            return Some(cancelled());
        };
        if !self
            .ensure_open_file_source_root_indexed(&file_path, true)
            .await
        {
            self.client
                .show_message(
                    MessageType::WARNING,
                    "Safe delete is unavailable until source-root indexing completes",
                )
                .await;
            return Some(cancelled());
        }
        if cancellation.is_cancelled() || !self.destructive_refactoring_generation_is_current() {
            return Some(cancelled());
        }

        let Ok(txn) = self.workspace.host.read_interactive().await else {
            return Some(cancelled());
        };
        let open_versions_before = open_document_versions(&self.documents.by_uri);
        let position = Position::new(line, character);
        let index = Arc::clone(&self.workspace.index);
        let blocking_source = source_text.clone();
        let blocking_uri = uri.clone();
        let blocking_cancellation = cancellation.clone();
        let proof = match txn
            .run_blocking(move |txn| {
                crate::code_actions::CodeActionProvider::new(txn, index)
                    .safe_delete_proof_cancellable(
                        file_id,
                        &blocking_uri,
                        &blocking_source,
                        position,
                        || blocking_cancellation.is_cancelled(),
                    )
            })
            .await
        {
            Ok(proof) => proof,
            Err(error) => {
                warn!("Safe delete proof task failed: {error}");
                None
            }
        };
        let Some(proof) = proof else {
            self.client
                .show_message(
                    MessageType::WARNING,
                    "Safe delete is no longer valid in the current workspace",
                )
                .await;
            return Some(cancelled());
        };
        if cancellation.is_cancelled() {
            return Some(cancelled());
        }

        let open_versions_after = open_document_versions(&self.documents.by_uri);
        let selected_is_current = self
            .documents
            .by_uri
            .get(uri.as_str())
            .is_some_and(|state| {
                state.version == version && state.text.as_str() == source_text.as_str()
            });
        if self.has_desynchronized_open_document()
            || !selected_is_current
            || open_versions_after != open_versions_before
        {
            self.client
                .show_message(
                    MessageType::WARNING,
                    "Safe delete was not applied because an open document changed",
                )
                .await;
            return Some(serde_json::json!({ "applied": false }));
        }
        let proof_workspace_revision = proof.workspace_revision;
        let proof_scanned_revisions = proof.scanned_revisions;
        let proof_resource = proof.resource_proof;
        let mut edit = proof.edit;
        if let Some(changes) = edit.changes.as_mut() {
            for guarded_uri in open_versions_before.keys() {
                changes.entry(guarded_uri.clone()).or_default();
            }
        }
        edit = fence_open_document_versions(edit, &open_versions_before);
        if self.supports_delete_file_workspace_edits() {
            edit = delete_emptied_source_file(edit, &uri, &source_text);
        }

        if cancellation.is_cancelled()
            || !self.supports_safe_delete_workspace_edits()
            || !self.destructive_refactoring_generation_is_current()
        {
            return Some(cancelled());
        }

        let validation_index = Arc::clone(&self.workspace.index);
        let validation_cancellation = cancellation.clone();
        let inputs_are_current = match tokio::task::spawn_blocking(move || {
            refactoring_inputs_are_current(
                &validation_index,
                proof_workspace_revision,
                &proof_scanned_revisions,
                proof_resource.as_deref(),
                &|| validation_cancellation.is_cancelled(),
            )
        })
        .await
        {
            Ok(current) => current,
            Err(error) => {
                warn!("Safe delete proof validation task failed: {error}");
                false
            }
        };
        if !inputs_are_current {
            return Some(cancelled());
        }

        // Filesystem validation awaited a blocking worker. Recheck every
        // async-owned fence afterward before sending the edit.
        let open_versions_after_validation = open_document_versions(&self.documents.by_uri);
        let selected_is_current = self
            .documents
            .by_uri
            .get(uri.as_str())
            .is_some_and(|state| {
                state.version == version && state.text.as_str() == source_text.as_str()
            });
        if cancellation.is_cancelled()
            || self.has_desynchronized_open_document()
            || !selected_is_current
            || open_versions_after_validation != open_versions_before
            || !self.supports_safe_delete_workspace_edits()
            || !self.destructive_refactoring_generation_is_current()
        {
            return Some(cancelled());
        }

        // Final application-time fence. The LSP has no compare-and-swap edit
        // primitive for closed files, so a watcher mutation after this check
        // but after the request is sent remains a protocol-level residual race.
        let applied = self
            .client
            .apply_edit(edit)
            .await
            .is_ok_and(|response| response.applied);
        if !applied {
            self.client
                .show_message(
                    MessageType::WARNING,
                    "The client rejected the safe delete edit",
                )
                .await;
        }
        Some(serde_json::json!({ "applied": applied }))
    }
}

/// When safe delete removed the last top-level type of `uri`, delete the file
/// instead of leaving a shell with only a package line and imports.
fn delete_emptied_source_file(
    mut edit: WorkspaceEdit,
    uri: &Url,
    source_text: &str,
) -> WorkspaceEdit {
    let Some(DocumentChanges::Edits(documents)) = edit.document_changes.as_ref() else {
        return edit;
    };
    let Some(target) = documents
        .iter()
        .find(|document| &document.text_document.uri == uri)
    else {
        return edit;
    };
    let edits = target
        .edits
        .iter()
        .map(|edit| match edit {
            OneOf::Left(edit) => edit.clone(),
            OneOf::Right(annotated) => annotated.text_edit.clone(),
        })
        .collect::<Vec<_>>();
    let remaining = crate::file_move::apply_text_edits(source_text, &edits);
    if !crate::file_move::declares_no_types(uri, &remaining) {
        return edit;
    }
    let Some(DocumentChanges::Edits(documents)) = edit.document_changes.take() else {
        return edit;
    };
    let mut operations = documents
        .into_iter()
        .filter(|document| &document.text_document.uri != uri)
        .map(DocumentChangeOperation::Edit)
        .collect::<Vec<_>>();
    operations.push(DocumentChangeOperation::Op(ResourceOp::Delete(
        tower_lsp::lsp_types::DeleteFile {
            uri: uri.clone(),
            options: None,
        },
    )));
    edit.document_changes = Some(DocumentChanges::Operations(operations));
    edit
}

fn code_action_kind_matches(requested: &CodeActionKind, actual: &CodeActionKind) -> bool {
    let requested = requested.as_str();
    let actual = actual.as_str();
    requested.is_empty()
        || actual == requested
        || actual
            .strip_prefix(requested)
            .is_some_and(|suffix| suffix.starts_with('.'))
}

fn refactoring_inputs_are_current<F>(
    workspace_index: &hyperion_workspace::WorkspaceIndex,
    workspace_revision: hyperion_workspace::WorkspaceRevision,
    scanned_revisions: &[hyperion_workspace::FileRevisionToken],
    resource_proof: Option<&hyperion_workspace::RefactoringResourceProof>,
    is_cancelled: &F,
) -> bool
where
    F: Fn() -> bool + ?Sized,
{
    let sources_are_current = || {
        !is_cancelled()
            && workspace_index
                .workspace_revisions_are_current(workspace_revision, scanned_revisions)
    };
    sources_are_current()
        && resource_proof.is_none_or(|proof| proof.is_current_cancellable(is_cancelled))
        && sources_are_current()
}

fn safe_delete_command_action(
    symbol_name: &str,
    uri: &Url,
    position: Position,
    version: i32,
) -> CodeAction {
    let title = format!("Safe Delete '{symbol_name}'…");
    CodeAction {
        title: title.clone(),
        kind: Some(CodeActionKind::REFACTOR),
        data: Some(serde_json::json!({ "hyperionSafeDelete": true })),
        command: Some(Command {
            title,
            command: SAFE_DELETE_COMMAND.to_string(),
            arguments: Some(vec![serde_json::json!({
                "uri": uri.as_str(),
                "line": position.line,
                "character": position.character,
                "version": version,
            })]),
        }),
        ..CodeAction::default()
    }
}

fn open_document_versions(
    documents: &Arc<dashmap::DashMap<String, crate::server::DocumentState>>,
) -> std::collections::HashMap<Url, i32> {
    documents
        .iter()
        .filter_map(|state| Url::parse(&state.uri).ok().map(|uri| (uri, state.version)))
        .collect()
}

fn fence_open_document_versions(
    mut workspace_edit: WorkspaceEdit,
    open_versions: &std::collections::HashMap<Url, i32>,
) -> WorkspaceEdit {
    let Some(changes) = workspace_edit.changes.take() else {
        return workspace_edit;
    };
    let mut changes = changes.into_iter().collect::<Vec<_>>();
    // Empty version guards must precede every mutation, then open/versioned
    // edits precede closed files. This gives non-transactional clients the
    // earliest possible stale-version rejection.
    changes.sort_unstable_by(|(left, left_edits), (right, right_edits)| {
        right_edits
            .is_empty()
            .cmp(&left_edits.is_empty())
            .then(
                open_versions
                    .contains_key(right)
                    .cmp(&open_versions.contains_key(left)),
            )
            .then(left.as_str().cmp(right.as_str()))
    });
    workspace_edit.document_changes = Some(DocumentChanges::Edits(
        changes
            .into_iter()
            .map(|(uri, edits)| TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    version: open_versions.get(&uri).copied(),
                    uri,
                },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            })
            .collect(),
    ));
    workspace_edit
}

pub(super) fn prepare_workspace_edit_for_client(
    mut workspace_edit: WorkspaceEdit,
    open_versions: &std::collections::HashMap<Url, i32>,
    supports_versioned_edits: bool,
    guard_all_open_documents: bool,
) -> WorkspaceEdit {
    if !supports_versioned_edits {
        return workspace_edit;
    }
    if guard_all_open_documents {
        if let Some(changes) = workspace_edit.changes.as_mut() {
            for uri in open_versions.keys() {
                changes.entry(uri.clone()).or_default();
            }
        }
    }
    fence_open_document_versions(workspace_edit, open_versions)
}

fn prepare_rename_workspace_edit_for_client(
    workspace_edit: WorkspaceEdit,
    open_versions: &std::collections::HashMap<Url, i32>,
    supports_versioned_edits: bool,
    supports_file_rename: bool,
    file_rename: Option<crate::rename::FileRenamePlan>,
) -> std::result::Result<WorkspaceEdit, String> {
    let Some(file_rename) = file_rename else {
        return Ok(prepare_workspace_edit_for_client(
            workspace_edit,
            open_versions,
            supports_versioned_edits,
            true,
        ));
    };
    if !supports_versioned_edits || !supports_file_rename {
        return Err(
            "Public top-level Java type rename requires versioned documentChanges and the Rename resource operation"
                .to_string(),
        );
    }

    let mut workspace_edit =
        prepare_workspace_edit_for_client(workspace_edit, open_versions, true, true);
    let Some(DocumentChanges::Edits(edits)) = workspace_edit.document_changes.take() else {
        return Err("Rename text edits could not be encoded as documentChanges".to_string());
    };
    let mut operations = edits
        .into_iter()
        .map(DocumentChangeOperation::Edit)
        .collect::<Vec<_>>();
    operations.push(DocumentChangeOperation::Op(ResourceOp::Rename(
        RenameFile {
            old_uri: file_rename.old_uri,
            new_uri: file_rename.new_uri,
            options: Some(RenameFileOptions {
                overwrite: Some(false),
                ignore_if_exists: Some(false),
            }),
            annotation_id: None,
        },
    )));
    workspace_edit.document_changes = Some(DocumentChanges::Operations(operations));
    Ok(workspace_edit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{Position, Range, TextEdit};

    #[test]
    fn code_action_only_matches_requested_kind_and_descendants() {
        assert!(code_action_kind_matches(
            &CodeActionKind::REFACTOR,
            &CodeActionKind::REFACTOR_EXTRACT
        ));
        assert!(code_action_kind_matches(
            &CodeActionKind::SOURCE,
            &CodeActionKind::SOURCE_ORGANIZE_IMPORTS
        ));
        assert!(!code_action_kind_matches(
            &CodeActionKind::QUICKFIX,
            &CodeActionKind::REFACTOR
        ));
        assert!(!code_action_kind_matches(
            &CodeActionKind::REFACTOR_EXTRACT,
            &CodeActionKind::REFACTOR
        ));
    }

    #[test]
    fn rename_edits_carry_captured_versions_for_open_documents() {
        let open = Url::parse("file:///workspace/Open.java").unwrap();
        let scanned_open = Url::parse("file:///workspace/Scanned.java").unwrap();
        let closed = Url::parse("file:///workspace/Closed.java").unwrap();
        let edit = TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(0, 3)),
            new_text: "new".to_string(),
        };
        let workspace_edit = WorkspaceEdit {
            changes: Some(std::collections::HashMap::from([
                (closed.clone(), vec![edit.clone()]),
                (open.clone(), vec![edit]),
            ])),
            document_changes: None,
            change_annotations: None,
        };

        let fenced = prepare_workspace_edit_for_client(
            workspace_edit,
            &std::collections::HashMap::from([(open.clone(), 17), (scanned_open.clone(), 23)]),
            true,
            true,
        );
        assert!(fenced.changes.is_none());
        let Some(DocumentChanges::Edits(edits)) = fenced.document_changes else {
            panic!("expected versioned text document edits");
        };
        assert_eq!(edits[0].text_document.uri, scanned_open);
        assert_eq!(edits[0].text_document.version, Some(23));
        assert!(edits[0].edits.is_empty());
        assert_eq!(edits[1].text_document.uri, open);
        assert_eq!(edits[1].text_document.version, Some(17));
        assert_eq!(edits[2].text_document.uri, closed);
        assert_eq!(edits[2].text_document.version, None);
    }

    #[test]
    fn clients_without_document_changes_keep_plain_local_edits() {
        let uri = Url::parse("file:///workspace/Local.java").unwrap();
        let edit = TextEdit {
            range: Range::new(Position::new(1, 2), Position::new(1, 5)),
            new_text: "renamed".to_string(),
        };
        let original = WorkspaceEdit {
            changes: Some(std::collections::HashMap::from([(uri.clone(), vec![edit])])),
            ..WorkspaceEdit::default()
        };

        let prepared = prepare_workspace_edit_for_client(
            original.clone(),
            &std::collections::HashMap::from([(uri, 9)]),
            false,
            true,
        );
        assert_eq!(prepared, original);
    }

    #[test]
    fn public_java_type_rename_orders_versioned_edits_before_file_operation() {
        let old_uri = Url::parse("file:///workspace/Target.java").unwrap();
        let new_uri = Url::parse("file:///workspace/Renamed.java").unwrap();
        let dependent_uri = Url::parse("file:///workspace/Use.java").unwrap();
        let edit = TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(0, 6)),
            new_text: "Renamed".to_string(),
        };
        let prepared = prepare_rename_workspace_edit_for_client(
            WorkspaceEdit {
                changes: Some(std::collections::HashMap::from([
                    (old_uri.clone(), vec![edit.clone()]),
                    (dependent_uri.clone(), vec![edit]),
                ])),
                ..WorkspaceEdit::default()
            },
            &std::collections::HashMap::from([(old_uri.clone(), 7)]),
            true,
            true,
            Some(crate::rename::FileRenamePlan {
                old_uri: old_uri.clone(),
                new_uri: new_uri.clone(),
            }),
        )
        .expect("protocol-capable client edit");

        assert!(prepared.changes.is_none());
        let Some(DocumentChanges::Operations(operations)) = prepared.document_changes else {
            panic!("expected mixed document changes");
        };
        assert_eq!(operations.len(), 3);
        assert!(matches!(operations[0], DocumentChangeOperation::Edit(_)));
        assert!(matches!(operations[1], DocumentChangeOperation::Edit(_)));
        let DocumentChangeOperation::Op(ResourceOp::Rename(rename)) = &operations[2] else {
            panic!("RenameFile must be the final operation");
        };
        assert_eq!(rename.old_uri, old_uri);
        assert_eq!(rename.new_uri, new_uri);
        assert_eq!(
            rename
                .options
                .as_ref()
                .and_then(|options| options.overwrite),
            Some(false)
        );
        let versioned_old = operations.iter().find_map(|operation| match operation {
            DocumentChangeOperation::Edit(edit) if edit.text_document.uri == old_uri => {
                Some(edit.text_document.version)
            }
            _ => None,
        });
        assert_eq!(versioned_old, Some(Some(7)));
    }

    #[test]
    fn public_java_type_rename_refuses_missing_protocol_capabilities() {
        let old_uri = Url::parse("file:///workspace/Target.java").unwrap();
        let new_uri = Url::parse("file:///workspace/Renamed.java").unwrap();
        for (document_changes, rename_operation) in [(false, false), (true, false), (false, true)] {
            let result = prepare_rename_workspace_edit_for_client(
                WorkspaceEdit {
                    changes: Some(std::collections::HashMap::from([(
                        old_uri.clone(),
                        Vec::new(),
                    )])),
                    ..WorkspaceEdit::default()
                },
                &std::collections::HashMap::new(),
                document_changes,
                rename_operation,
                Some(crate::rename::FileRenamePlan {
                    old_uri: old_uri.clone(),
                    new_uri: new_uri.clone(),
                }),
            );
            assert!(
                result.is_err(),
                "documentChanges={document_changes}, rename={rename_operation}"
            );
        }
    }

    #[test]
    fn safe_delete_command_action_carries_revision_checked_payload() {
        let uri = Url::parse("file:///workspace/Entity.java").unwrap();
        let action = safe_delete_command_action("dead", &uri, Position::new(3, 9), 12);
        assert!(action.edit.is_none());
        let command = action.command.expect("deferred command");
        assert_eq!(command.command, SAFE_DELETE_COMMAND);
        assert_eq!(
            command.arguments.unwrap()[0],
            serde_json::json!({
                "uri": "file:///workspace/Entity.java",
                "line": 3,
                "character": 9,
                "version": 12,
            })
        );
    }

    #[test]
    fn empty_version_guards_precede_destructive_edits() {
        let guard = Url::parse("file:///workspace/ZGuard.java").unwrap();
        let destructive = Url::parse("file:///workspace/ADelete.java").unwrap();
        let edit = TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(0, 3)),
            new_text: String::new(),
        };
        let fenced = fence_open_document_versions(
            WorkspaceEdit {
                changes: Some(std::collections::HashMap::from([
                    (destructive.clone(), vec![edit]),
                    (guard.clone(), Vec::new()),
                ])),
                ..WorkspaceEdit::default()
            },
            &std::collections::HashMap::from([(guard.clone(), 3), (destructive.clone(), 5)]),
        );
        let Some(DocumentChanges::Edits(edits)) = fenced.document_changes else {
            panic!("expected document changes");
        };
        assert_eq!(edits[0].text_document.uri, guard);
        assert!(edits[0].edits.is_empty());
        assert_eq!(edits[1].text_document.uri, destructive);
    }
}
