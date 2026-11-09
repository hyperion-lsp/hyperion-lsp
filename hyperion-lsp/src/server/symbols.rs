// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use hyperion_workspace::Language;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    DocumentSymbolParams, DocumentSymbolResponse, SymbolInformation, WorkspaceSymbolParams,
};
use tower_lsp::CancellationToken;
use tracing::{debug, warn};

use crate::language_handler::LanguageHandlerContext;
use crate::server::Backend;

impl Backend {
    pub(crate) async fn handle_document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Option<DocumentSymbolResponse> {
        let uri = params.text_document.uri;

        debug!("Document symbol request for {}", uri);

        let (file_id, source_text, language) =
            if let Some(state) = self.documents.by_uri.get(uri.as_str()) {
                (state.file_id, state.text.clone(), state.language)
            } else {
                warn!("Document symbols requested for unopened document: {}", uri);
                return None;
            };

        // Java's outline is built from the editor buffer alone, so it needs no
        // semantic transaction and answers even while a writer is queued.
        // Every other language reads the semantic database: Kotlin has its own
        // CST and must not be walked with the Java SyntaxKind enum.
        let result = if language == Language::Java {
            self.services
                .symbols
                .java_document_symbols_from_source(&source_text)
        } else {
            let txn = self.workspace.host.read_interactive().await.ok()?;
            let ctx = LanguageHandlerContext {
                txn: &txn,
                workspace_index: &self.workspace.index,
                source_providers: &self.services.source_providers,
                query_engine: None,
            };
            ctx.handler_for(language)
                .and_then(|handler| handler.document_symbols(file_id, &source_text))
                .unwrap_or_else(|| {
                    self.services
                        .symbols
                        .document_symbols(&txn, file_id, &source_text)
                })
        };

        let symbols = match result {
            Ok(syms) => Some(syms),
            Err(e) => {
                debug!("Document symbols failed: {}", e);
                None
            }
        };
        debug!(
            "Returning {} document symbols",
            symbols.as_ref().map_or(0, std::vec::Vec::len)
        );
        symbols.map(DocumentSymbolResponse::Nested)
    }

    pub(crate) fn handle_workspace_symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let cancellation = CancellationToken::current();
        cancellation.check()?;
        debug!("Workspace symbol search for query: '{}'", params.query);

        // Use query engine to search workspace
        let result = self
            .workspace
            .query
            .workspace_symbols_cancellable(&params.query, || cancellation.is_cancelled());
        cancellation.check()?;
        match result {
            Ok(symbols) => {
                debug!("Found {} workspace symbols", symbols.len());
                Ok(Some(symbols))
            }
            Err(err) => {
                warn!("Workspace symbol search failed: {}", err);
                Ok(None)
            }
        }
    }
}
