// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::Arc;

use hyperion_workspace::Language;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionList, CompletionParams, CompletionResponse, Hover, HoverParams, InlayHint,
    SignatureHelp, SignatureHelpParams, Url,
};
use tower_lsp::CancellationToken;
use tracing::{debug, warn};

use crate::language_handler::LanguageHandlerContext;
use crate::server::Backend;

/// Result for a completion that failed for a *transient* reason: a contended DB lock, an
/// exhausted interactive permit, or a `didChange` that landed mid-request.
///
/// Returning `None` here would be a definitive "there are no completions at this position",
/// and clients cache that: Neovim in particular issues one request per trigger and reuses
/// the result until the context changes, so a single 2s timeout leaves the user staring at
/// an empty popup until they retype. An empty list marked `is_incomplete` says "nothing
/// *yet*, ask me again", which is the same signal the success path already sends.
const fn retry_completion() -> Option<CompletionResponse> {
    Some(CompletionResponse::List(CompletionList {
        is_incomplete: true,
        items: Vec::new(),
    }))
}

impl Backend {
    pub(crate) async fn handle_completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let cancellation = CancellationToken::current();
        cancellation.check()?;

        // Cancellation checkpoint: Allow tower-lsp to abort this request if $/cancelRequest was sent
        tokio::task::yield_now().await;
        cancellation.check()?;

        let Ok(txn) = self.workspace.host.read_interactive().await else {
            debug!("Completion: semantic state is busy; asking client to retry");
            return Ok(retry_completion());
        };
        cancellation.check()?;

        let uri = params.text_document_position.text_document.uri.to_string();
        let position = params.text_document_position.position;

        // BUG FIX #16 (v2): Use version counter to prevent TOCTOU race without nested locks.
        //
        // Previous approach held the DashMap entry guard while acquiring db.lock(), which
        // risks deadlock if any future code path acquires locks in the opposite order.
        //
        // New approach: Read text + change_version, drop DashMap guard, acquire db lock,
        // then re-check change_version. If it changed, a didChange() interleaved -- abort.
        let (file_id, change_version) = if let Some(doc_state) = self.documents.by_uri.get(&uri) {
            (doc_state.file_id, doc_state.change_version)
        } else {
            debug!("Document not found: {}", uri);
            return Ok(None);
        };

        // Race check: if didChange() ran between our document_map read and now,
        // the change_version will have incremented -- abort to avoid stale data.
        // Note: didChange already sets file_text/file_language in Salsa, so we skip
        // the redundant db.set_file_text() here (eliminates one db.lock() per completion).
        if let Some(current) = self.documents.by_uri.get(&uri) {
            if current.change_version != change_version {
                debug!(
                    "Completion aborted: document changed (v{} -> v{})",
                    change_version, current.change_version
                );
                return Ok(retry_completion());
            }
        }

        // Get completions from the provider
        let result =
            self.services
                .completion
                .complete_cancellable(&txn, file_id, position, &cancellation);
        cancellation.check()?;
        match result {
            Ok(items) => {
                debug!("Completion request returned {} items", items.len());
                // Use CompletionList with is_incomplete=true to tell client to re-query
                // as user types more characters. This fixes the issue where Neovim
                // only sends one completion request and reuses stale results.
                Ok(Some(CompletionResponse::List(CompletionList {
                    is_incomplete: true,
                    items,
                })))
            }
            Err(e) => {
                // Includes "DB lock contended" from the provider's `try_lock_for`, which is
                // transient by construction. The success path already sets `is_incomplete`,
                // so the client re-queries on the next keystroke either way.
                debug!("Completion error: {}", e);
                Ok(retry_completion())
            }
        }
    }

    pub(crate) async fn handle_hover(&self, params: HoverParams) -> Option<Hover> {
        // Cancellation checkpoint: Allow tower-lsp to abort this request if $/cancelRequest was sent
        tokio::task::yield_now().await;

        let txn = self.workspace.host.read_interactive().await.ok()?;

        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        debug!("Hover request for {} at {:?}", uri, position);

        // Get the document's FileId and text
        let (file_id, source_text) =
            if let Some(doc_state) = self.documents.by_uri.get(uri.as_str()) {
                (doc_state.file_id, doc_state.text.clone())
            } else {
                warn!("Hover requested for unopened document: {}", uri);
                return None;
            };

        // Check if there's a security finding at this position first
        // Security hovers take precedence as they provide critical vulnerability information
        if let Some(security_hover) =
            self.services
                .security
                .hover_at_position(&uri, position, &source_text)
        {
            debug!("Returning security hover information");
            return Some(security_hover);
        }

        // Get hover information from the provider
        match self
            .hover_provider(&txn)
            .hover(file_id, position, &source_text)
        {
            Ok(hover) => {
                debug!("Returning hover information: {:?}", hover.is_some());
                hover
            }
            Err(e) => {
                warn!("Hover error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Option<SignatureHelp> {
        // Cancellation checkpoint: Allow tower-lsp to abort this request if $/cancelRequest was sent
        tokio::task::yield_now().await;

        let txn = self.workspace.host.read_interactive().await.ok()?;

        let document_uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let uri = document_uri.to_string();
        let position = params.text_document_position_params.position;

        debug!("Signature help request for {} at {:?}", uri, position);

        // Get the document's FileId and text
        let (file_id, source_text) = if let Some(doc_state) = self.documents.by_uri.get(&uri) {
            (doc_state.file_id, doc_state.text.clone())
        } else {
            warn!("Signature help requested for unopened document: {}", uri);
            return None;
        };

        // Per-language dispatch, same convention as document symbols: a
        // language with its own CST answers for itself, Java (and anything
        // without an override) falls through to `signature_help_provider`.
        let ctx = LanguageHandlerContext {
            txn: &txn,
            workspace_index: &self.workspace.index,
            source_providers: &self.services.source_providers,
            query_engine: None,
        };

        let result = ctx
            .handler_for(language_of(&document_uri))
            .and_then(|handler| handler.signature_help(file_id, position, &source_text))
            .unwrap_or_else(|| {
                crate::signature_help::SignatureHelpProvider::new(
                    &txn,
                    Arc::clone(&self.workspace.index),
                )
                .signature_help(file_id, position, &source_text)
            });

        match result {
            Ok(signature_help) => {
                debug!("Returning signature help: {:?}", signature_help.is_some());
                signature_help
            }
            Err(e) => {
                warn!("Signature help error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_inlay_hint(
        &self,
        params: tower_lsp::lsp_types::InlayHintParams,
    ) -> Option<Vec<InlayHint>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        let uri = params.text_document.uri.to_string();
        let range = params.range;

        debug!("Inlay hint request for {} in range {:?}", uri, range);

        // Get the document's FileId and text
        let (file_id, source_text) = if let Some(doc_state) = self.documents.by_uri.get(&uri) {
            (doc_state.file_id, doc_state.text.clone())
        } else {
            warn!("Inlay hint requested for unopened document: {}", uri);
            return None;
        };

        // Per-language dispatch (see `handle_signature_help`).
        let ctx = LanguageHandlerContext {
            txn: &txn,
            workspace_index: &self.workspace.index,
            source_providers: &self.services.source_providers,
            query_engine: None,
        };

        let result = ctx
            .handler_for(language_of(&params.text_document.uri))
            .and_then(|handler| handler.inlay_hints(file_id, range, &source_text))
            .unwrap_or_else(|| {
                crate::inlay_hints::InlayHintsProvider::new(&txn, Arc::clone(&self.workspace.index))
                    .inlay_hints(file_id, range, &source_text)
            });

        match result {
            Ok(hints) => {
                debug!("Returning {} inlay hints", hints.len());
                Some(hints)
            }
            Err(e) => {
                warn!("Inlay hints error: {}", e);
                None
            }
        }
    }
}

/// The language of a document, derived from its URI.
///
/// This repeats verbatim what `handle_did_open` computes before writing
/// `file_language` into Salsa, so it agrees with the DB by construction and
/// needs no snapshot.
pub(crate) fn language_of(uri: &Url) -> Language {
    uri.to_file_path()
        .ok()
        .and_then(|path| Language::from_path(&path))
        .unwrap_or(Language::Java)
}
