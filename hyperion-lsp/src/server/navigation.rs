// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::sync::atomic::Ordering;
use std::sync::Arc;

use hyperion_workspace::WorkspaceIndex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    DocumentHighlight, DocumentHighlightParams, GotoDefinitionParams, GotoDefinitionResponse,
    Location, ReferenceParams,
};
use tower_lsp::CancellationToken;
use tracing::{debug, info, warn};

use super::state::DocumentStore;
use crate::semantic_host::ReadTxn;
use crate::server::Backend;

/// One document generation that is coherent across the editor buffer, the
/// workspace index, and the Salsa mirror, captured inside a read transaction.
///
/// Every publication of those three (didOpen/didChange/didClose, watcher
/// reconciliation, bulk and external indexing) runs in the exclusive write
/// transaction, so nothing can move while the request holds its read
/// transaction. That makes one capture sufficient: the retry-to-coherence
/// loop, its 100 ms wait, and the per-request revision counters this used to
/// need existed only because publication could interleave with a request.
#[derive(Clone)]
struct NavigationRequest {
    file_id: hyperion_semantics::db::FileId,
    source_text: Arc<String>,
}

impl NavigationRequest {
    /// Coherence is exact for *this* file: the editor text, the indexed
    /// publication that owns the ranges, and the Salsa mirror must be the same
    /// revision. `None` means the document is unopened or a failed publication
    /// left it desynchronized; navigation must not guess in that case.
    fn capture(
        documents: &DocumentStore,
        index: &WorkspaceIndex,
        txn: &ReadTxn,
        uri: &tower_lsp::lsp_types::Url,
    ) -> Option<Self> {
        let (file_id, source_text) = documents
            .by_uri
            .get(uri.as_str())
            .map(|state| (state.file_id, Arc::clone(&state.text)))?;
        let file_revision = index.current_file_revision(file_id)?;
        let indexed = index.get_indexed_revision_content(file_revision)?;
        let request_path = uri.to_file_path().ok()?;
        if indexed.file_path != request_path || indexed.content.as_str() != source_text.as_str() {
            return None;
        }
        let semantic_source = txn.try_file_text(file_id)?;
        Arc::ptr_eq(&semantic_source, &source_text).then_some(Self {
            file_id,
            source_text,
        })
    }
}

impl Backend {
    fn capture_navigation_request(
        &self,
        txn: &ReadTxn,
        uri: &tower_lsp::lsp_types::Url,
    ) -> Option<NavigationRequest> {
        NavigationRequest::capture(&self.documents, &self.workspace.index, txn, uri)
    }
}

impl Backend {
    pub(crate) async fn handle_goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Option<GotoDefinitionResponse> {
        // Cancellation checkpoint: Allow tower-lsp to abort this request if $/cancelRequest was sent
        tokio::task::yield_now().await;

        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        debug!("Goto definition request for {} at {:?}", uri, position);

        // Index the file's source root in the background as soon as navigation
        // is used and the workspace is ready (didOpen usually arrives before
        // that). Otherwise the first find-references pays the whole wait for
        // this indexing (3 s on a large class). No transaction may be held
        // here: that indexing takes the write transaction.
        if let Ok(path) = uri.to_file_path() {
            self.maybe_index_open_file_source_root(&path).await;
        }

        let txn = self.workspace.host.read_interactive().await.ok()?;
        let location = self
            .capture_navigation_request(&txn, &uri)
            .and_then(|request| {
                match self.navigation_provider(&txn).goto_definition(
                    request.file_id,
                    position,
                    &request.source_text,
                    &uri,
                ) {
                    Ok(location) => location,
                    Err(error) => {
                        debug!("Goto definition failed: {error}");
                        None
                    }
                }
            });

        if location.is_none() {
            info!(
                "NAV: goto-definition empty for {} at {:?} (workspace_ready={}, {})",
                uri,
                position,
                self.workspace.ready.load(Ordering::Relaxed),
                self.workspace.index.get_index_diagnostic()
            );
        } else {
            debug!("Returning goto definition: true");
        }
        location.map(GotoDefinitionResponse::Scalar)
    }

    pub(crate) async fn handle_goto_implementation(
        &self,
        params: GotoDefinitionParams,
    ) -> Option<GotoDefinitionResponse> {
        // Cancellation checkpoint: Allow tower-lsp to abort this request if $/cancelRequest was sent
        tokio::task::yield_now().await;

        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        debug!("Goto implementation request for {} at {:?}", uri, position);

        let txn = self.workspace.host.read_interactive().await.ok()?;
        let request = self.capture_navigation_request(&txn, &uri)?;
        let language = crate::server::code_intel::language_of(&uri);
        let hierarchy_context = self.hierarchy_context(&txn);
        let navigation = self.navigation_provider(&txn);
        if let Some(result) = hierarchy_context.handler_for(language).and_then(|handler| {
            handler.goto_implementation(request.file_id, position, &request.source_text)
        }) {
            match result {
                Ok(Some(locations)) => {
                    crate::handlers::navigation::locations_to_goto_response(locations)
                }
                Ok(None) => navigation
                    .goto_definition(request.file_id, position, &request.source_text, &uri)
                    .ok()
                    .flatten()
                    .map(GotoDefinitionResponse::Scalar),
                Err(error) => {
                    debug!("Language-specific goto implementation failed: {error}");
                    None
                }
            }
        } else if language != hyperion_workspace::Language::Java {
            // A non-Java handler that declines this opt-in capability must not
            // feed its green tree to Java's parser. Preserve goto-definition
            // fallback.
            navigation
                .goto_definition(request.file_id, position, &request.source_text, &uri)
                .ok()
                .flatten()
                .map(GotoDefinitionResponse::Scalar)
        } else {
            crate::handlers::navigation::resolve_goto_implementation(
                &navigation,
                &self.services.type_hierarchy,
                request.file_id,
                position,
                &request.source_text,
                &uri,
            )
        }
    }

    pub(crate) async fn handle_references(
        &self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<Location>>> {
        let cancellation = CancellationToken::current();
        cancellation.check()?;

        // Cancellation checkpoint: Allow tower-lsp to abort this request if $/cancelRequest was sent
        tokio::task::yield_now().await;
        cancellation.check()?;

        let uri = params.text_document_position.text_document.uri.clone();
        let position = params.text_document_position.position;

        if let Ok(file_path) = uri.to_file_path() {
            // Best-effort: wait briefly for the opened-file source-root index. This avoids
            // returning incomplete results right after didOpen on cold start.
            //
            // No transaction may be held here: that index takes the write
            // transaction, so waiting with a read transaction held would
            // always run into the full timeout.
            self.ensure_open_file_source_root_indexed(&file_path, true)
                .await;
            cancellation.check()?;
        }

        let Ok(txn) = self.workspace.host.read_interactive().await else {
            return Ok(None);
        };
        cancellation.check()?;

        debug!("Find references request for {} at {:?}", uri, position);

        if !self.documents.by_uri.contains_key(uri.as_str()) {
            warn!("Find references requested for unopened document: {}", uri);
            return Ok(None);
        }

        let locations = self
            .capture_navigation_request(&txn, &uri)
            .and_then(|request| {
                match self.navigation_provider(&txn).find_references_cancellable(
                    request.file_id,
                    position,
                    &request.source_text,
                    &uri,
                    params.context.include_declaration,
                    &cancellation,
                ) {
                    Ok(locations) => Some(locations),
                    Err(error) => {
                        debug!("Find references failed: {error}");
                        None
                    }
                }
            });
        cancellation.check()?;

        let count = locations.as_ref().map_or(0, std::vec::Vec::len);
        if count == 0 {
            info!(
                "NAV: references empty for {} at {:?} (include_declaration={}, workspace_ready={}, {})",
                uri,
                position,
                params.context.include_declaration,
                self.workspace.ready.load(Ordering::Relaxed),
                self.workspace.index.get_index_diagnostic()
            );
        } else {
            debug!("Returning {} reference locations", count);
        }
        Ok(locations)
    }

    pub(crate) async fn handle_document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Option<Vec<DocumentHighlight>> {
        // Cancellation checkpoint: Allow tower-lsp to abort this request if $/cancelRequest was sent
        tokio::task::yield_now().await;

        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        debug!("Document highlight request for {} at {:?}", uri, position);

        let txn = self.workspace.host.read_interactive().await.ok()?;
        let locations = self
            .capture_navigation_request(&txn, &uri)
            .and_then(|request| {
                match self.navigation_provider(&txn).find_references(
                    request.file_id,
                    position,
                    &request.source_text,
                    &uri,
                    true,
                ) {
                    Ok(locations) => Some(locations),
                    Err(error) => {
                        debug!("Document highlight find_references failed: {error}");
                        None
                    }
                }
            });

        let highlights =
            crate::handlers::navigation::document_highlights_from_references(locations, &uri);

        debug!(
            "Returning {} document highlights",
            highlights.as_ref().map_or(0, std::vec::Vec::len)
        );
        highlights
    }
}

#[cfg(test)]
mod navigation_request_tests {
    use super::*;
    use crate::semantic_host::SemanticHost;
    use crate::server::state::DocumentState;
    use hyperion_workspace::Language;

    fn fixture(
        source: &str,
    ) -> (
        tempfile::TempDir,
        DocumentStore,
        Arc<SemanticHost>,
        tower_lsp::lsp_types::Url,
    ) {
        let temp = tempfile::tempdir().expect("temporary workspace");
        let path = temp.path().join("Client.java");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&path, source)
            .expect("indexed source");
        let file_id = index.get_file_id(&path).expect("file id");
        let uri = tower_lsp::lsp_types::Url::from_file_path(&path).expect("file URI");
        let source_text = Arc::new(source.to_owned());
        let host = SemanticHost::new(index);
        host.write_now()
            .db()
            .register_file(file_id, Arc::clone(&source_text), Language::Java);
        let documents = DocumentStore::new();
        documents.by_uri.insert(
            uri.as_str().to_owned(),
            DocumentState {
                uri: uri.as_str().to_owned(),
                language_id: "java".to_owned(),
                language: Language::Java,
                version: 1,
                desynchronized: false,
                text: Arc::clone(&source_text),
                file_id,
                change_version: 11,
            },
        );
        (temp, documents, host, uri)
    }

    #[test]
    fn a_coherent_document_is_captured() {
        let (_temp, documents, host, uri) = fixture("class Client {}");
        let txn = host.read_now();
        let request = NavigationRequest::capture(&documents, host.index(), &txn, &uri)
            .expect("coherent request");
        assert_eq!(request.source_text.as_str(), "class Client {}");
    }

    #[test]
    fn an_editor_buffer_that_differs_from_the_index_is_not_captured() {
        let (_temp, documents, host, uri) = fixture("class Client {}");
        {
            let mut document = documents.by_uri.get_mut(uri.as_str()).expect("document");
            document.text = Arc::new("class Client { int changed; }".to_owned());
        }
        let txn = host.read_now();
        assert!(NavigationRequest::capture(&documents, host.index(), &txn, &uri).is_none());
    }

    #[test]
    fn a_split_semantic_mirror_is_not_captured() {
        let (_temp, documents, host, uri) = fixture("class Client {}");
        let file_id = documents.by_uri.get(uri.as_str()).unwrap().file_id;
        // Same bytes, different allocation: the Salsa mirror is not the buffer.
        host.write_now()
            .db()
            .set_file_text_tracked(file_id, Arc::new("class Client {}".to_owned()));
        let txn = host.read_now();
        assert!(NavigationRequest::capture(&documents, host.index(), &txn, &uri).is_none());
    }
}
