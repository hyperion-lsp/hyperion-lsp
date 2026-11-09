// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    TypeHierarchyItem, TypeHierarchyPrepareParams, TypeHierarchySubtypesParams,
    TypeHierarchySupertypesParams,
};
use tracing::{debug, warn};

use crate::language_handler::LanguageHandlerContext;
use crate::semantic_host::ReadTxn;
use crate::server::code_intel::language_of;
use crate::server::Backend;

impl Backend {
    /// Per-request dispatch context. A language with its own CST answers for
    /// itself or fails closed; only Java reaches the Java providers.
    pub(super) const fn hierarchy_context<'a>(
        &'a self,
        txn: &'a ReadTxn,
    ) -> LanguageHandlerContext<'a> {
        LanguageHandlerContext {
            txn,
            workspace_index: &self.workspace.index,
            source_providers: &self.services.source_providers,
            query_engine: None,
        }
    }

    pub(crate) async fn handle_prepare_type_hierarchy(
        &self,
        params: TypeHierarchyPrepareParams,
    ) -> Option<Vec<TypeHierarchyItem>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        let document_uri = params
            .text_document_position_params
            .text_document
            .uri
            .clone();
        let uri = document_uri.to_string();
        let position = params.text_document_position_params.position;

        debug!(
            "Prepare type hierarchy request for {} at {:?}",
            uri, position
        );

        let (file_id, source_text) = if let Some(doc_state) = self.documents.by_uri.get(&uri) {
            (doc_state.file_id, doc_state.text.clone())
        } else {
            warn!("Type hierarchy requested for unopened document: {}", uri);
            return None;
        };

        let ctx = self.hierarchy_context(&txn);
        let result = ctx.dispatch_opt_in(
            language_of(&document_uri),
            |handler| handler.prepare_type_hierarchy(file_id, position, &source_text),
            || {
                self.services
                    .type_hierarchy
                    .prepare_type_hierarchy(file_id, position, &source_text)
            },
            || Ok(None),
        );

        match result {
            Ok(items) => {
                debug!("Returning type hierarchy items: {:?}", items.is_some());
                items
            }
            Err(e) => {
                warn!("Type hierarchy error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_supertypes(
        &self,
        params: TypeHierarchySupertypesParams,
    ) -> Option<Vec<TypeHierarchyItem>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        debug!(
            "Type hierarchy supertypes request for {:?}",
            params.item.name
        );

        // The item, not an open document, carries the language here: a
        // hierarchy can be walked far away from where it was prepared.
        let ctx = self.hierarchy_context(&txn);
        let result = ctx.dispatch_opt_in(
            language_of(&params.item.uri),
            |handler| handler.supertypes(&params.item),
            || self.services.type_hierarchy.supertypes(&params.item),
            || Ok(None),
        );

        match result {
            Ok(items) => {
                debug!("Returning supertypes: {:?}", items.is_some());
                items
            }
            Err(e) => {
                warn!("Supertypes error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_subtypes(
        &self,
        params: TypeHierarchySubtypesParams,
    ) -> Option<Vec<TypeHierarchyItem>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        debug!("Type hierarchy subtypes request for {:?}", params.item.name);

        let ctx = self.hierarchy_context(&txn);
        let result = ctx.dispatch_opt_in(
            language_of(&params.item.uri),
            |handler| handler.subtypes(&params.item),
            || self.services.type_hierarchy.subtypes(&params.item),
            || Ok(None),
        );

        match result {
            Ok(items) => {
                debug!("Returning subtypes: {:?}", items.is_some());
                items
            }
            Err(e) => {
                warn!("Subtypes error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Option<Vec<CallHierarchyItem>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        debug!(
            "Prepare call hierarchy request for {} at {:?}",
            uri, position
        );

        let (file_id, source_text) =
            if let Some(doc_state) = self.documents.by_uri.get(uri.as_str()) {
                (doc_state.file_id, doc_state.text.clone())
            } else {
                warn!("Call hierarchy requested for unopened document: {}", uri);
                return None;
            };

        let ctx = self.hierarchy_context(&txn);
        let result = ctx.dispatch_opt_in(
            language_of(uri),
            |handler| handler.prepare_call_hierarchy(file_id, position, &source_text, uri),
            || {
                self.call_hierarchy_provider(&txn)
                    .prepare(file_id, position, &source_text, uri)
            },
            || Ok(None),
        );

        match result {
            Ok(items) => {
                debug!("Returning call hierarchy items: {:?}", items.is_some());
                items
            }
            Err(e) => {
                warn!("Call hierarchy prepare error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Option<Vec<CallHierarchyIncomingCall>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        debug!("Incoming calls request for {:?}", params.item.name);

        let ctx = self.hierarchy_context(&txn);
        let result = ctx.dispatch_opt_in(
            language_of(&params.item.uri),
            |handler| handler.incoming_calls(&params.item),
            || {
                self.call_hierarchy_provider(&txn)
                    .incoming_calls(&params.item)
            },
            || Ok(Vec::new()),
        );

        match result {
            Ok(calls) => {
                debug!("Returning {} incoming calls", calls.len());
                if calls.is_empty() {
                    None
                } else {
                    Some(calls)
                }
            }
            Err(e) => {
                warn!("Incoming calls error: {}", e);
                None
            }
        }
    }

    pub(crate) async fn handle_outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Option<Vec<CallHierarchyOutgoingCall>> {
        let txn = self.workspace.host.read_interactive().await.ok()?;
        debug!("Outgoing calls request for {:?}", params.item.name);

        let ctx = self.hierarchy_context(&txn);
        let result = ctx.dispatch_opt_in(
            language_of(&params.item.uri),
            |handler| handler.outgoing_calls(&params.item),
            || {
                self.call_hierarchy_provider(&txn)
                    .outgoing_calls(&params.item)
            },
            || Ok(Vec::new()),
        );

        match result {
            Ok(calls) => {
                debug!("Returning {} outgoing calls", calls.len());
                if calls.is_empty() {
                    None
                } else {
                    Some(calls)
                }
            }
            Err(e) => {
                warn!("Outgoing calls error: {}", e);
                None
            }
        }
    }
}
