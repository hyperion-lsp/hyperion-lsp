// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! LSP Server Implementation for Project Hyperion
//!
//! This module implements the Language Server Protocol (LSP) server for Project Hyperion,
//! a high-performance JVM language server written in Rust. The server architecture follows
//! the LSP specification and provides intelligent IDE features for Java and other JVM languages.
//!
//! # Architecture Overview
//!
//! The LSP server is built on the following architectural principles:
//!
//! 1. **Async/Await Model**: Uses `tokio` for asynchronous I/O and `tower-lsp` for LSP protocol handling
//! 2. **State Management**: Maintains document state in memory with efficient synchronization
//! 3. **Incremental Updates**: Supports incremental document updates for optimal performance
//! 4. **Query-Based Architecture**: Will integrate with Salsa for incremental computation
//!
//! ## LSP Lifecycle
//!
//! The server follows the standard LSP lifecycle:
//! 1. Client sends `initialize` request → Server responds with capabilities
//! 2. Client sends `initialized` notification → Server is now ready
//! 3. Client sends document notifications (`didOpen`, `didChange`, `didClose`)
//! 4. Client sends requests (`completion`, `hover`, `definition`, etc.)
//! 5. Client sends `shutdown` request → Server prepares to exit
//! 6. Client sends `exit` notification → Server terminates
//!
//! ## Server Capabilities
//!
//! The current implementation declares the following capabilities:
//! - **Text Document Sync**: Full document synchronization with incremental updates
//! - **Position Encoding**: UTF-16 code units (LSP standard)
//!
//! Future capabilities will include:
//! - Code completion with context-aware suggestions
//! - Hover information with type and documentation
//! - Go to definition, implementation, and type definition
//! - Find references and document highlights
//! - Code actions and quick fixes
//! - Rename and refactoring operations
//! - Semantic tokens for syntax highlighting
//! - Inlay hints for parameter names and types
//! - Call hierarchy and type hierarchy
//! - Diagnostics with compiler-quality error messages

use dashmap::DashMap;
use hyperion_semantics::db::FileId;
use hyperion_workspace::{Language, QueryEngine, WorkspaceIndex, WorkspaceRevision};
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    CallHierarchyServerCapability, CodeActionKind, CodeActionOptions, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CodeLens, CodeLensOptions, CodeLensParams,
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentFormattingParams,
    DocumentHighlight, DocumentHighlightParams, DocumentLink, DocumentLinkOptions,
    DocumentLinkParams, DocumentRangeFormattingParams, DocumentSymbolParams,
    DocumentSymbolResponse, ExecuteCommandOptions, ExecuteCommandParams, FoldingRange,
    FoldingRangeParams, FoldingRangeProviderCapability, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverParams, HoverProviderCapability,
    ImplementationProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    InlayHint, Location, OneOf, PositionEncodingKind, PrepareRenameResponse, ReferenceParams,
    Registration, RenameOptions, RenameParams, ResourceOperationKind, SaveOptions,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, ServerInfo, SignatureHelp,
    SignatureHelpOptions, SignatureHelpParams, SymbolInformation, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, TextEdit, TypeHierarchyItem, TypeHierarchyPrepareParams,
    TypeHierarchySubtypesParams, TypeHierarchySupertypesParams, Url, WillSaveTextDocumentParams,
    WorkDoneProgressOptions, WorkspaceEdit, WorkspaceFoldersServerCapabilities,
    WorkspaceServerCapabilities, WorkspaceSymbolParams,
};
use tower_lsp::{Client, LanguageServer, LspService};
use tracing::{debug, info, warn};

use crate::commands::supported_commands;
use crate::completion::CompletionProvider;
use crate::hover::HoverProvider;
use crate::navigation::NavigationProvider;
use crate::semantic_host::{ReadTxn, SemanticHost};
use crate::symbols::SymbolsProvider;

mod code_intel;
pub mod diagnostics;
mod document_sync;
mod file_operations;
mod formatting;
mod hierarchy;
mod misc;
mod navigation;
mod refactoring;
pub(crate) mod source_indexing;
mod state;
pub(crate) use state::{
    DestructiveRefactoringReadiness, RefactoringIndexComponent, RefactoringReadinessTicket,
    SourceMutationReadinessGuard,
};
mod symbols;

pub use state::{Backend, DocumentState};
use state::{
    BackgroundTasks, DiagnosticAnalysisTicket, DiagnosticPublicationState, DocumentStore,
    IndexGate, LanguageServices, WorkspaceAnalysis,
};

/// Language providers that read semantic state are request-scoped: each borrows
/// the request's [`ReadTxn`], the only source of snapshots, so none can outlive
/// or bypass the gate. They are cheap to build (a few `Arc` clones).
impl Backend {
    pub(crate) fn navigation_provider<'txn>(&self, txn: &'txn ReadTxn) -> NavigationProvider<'txn> {
        NavigationProvider::new(
            txn,
            Arc::clone(&self.workspace.index),
            Arc::clone(&self.workspace.query),
            Arc::clone(&self.services.source_providers),
        )
    }

    pub(crate) fn hover_provider<'txn>(&self, txn: &'txn ReadTxn) -> HoverProvider<'txn> {
        HoverProvider::with_query_engine(
            txn,
            Arc::clone(&self.workspace.index),
            Arc::clone(&self.workspace.query),
            Arc::clone(&self.services.source_providers),
        )
    }

    pub(crate) fn call_hierarchy_provider<'txn>(
        &self,
        txn: &'txn ReadTxn,
    ) -> crate::call_hierarchy::CallHierarchyProvider<'txn> {
        crate::call_hierarchy::CallHierarchyProvider::new(txn, Arc::clone(&self.workspace.index))
    }
}

impl Backend {
    pub(crate) fn has_desynchronized_open_document(&self) -> bool {
        self.documents
            .by_uri
            .iter()
            .any(|state| state.desynchronized)
    }

    /// Return the immutable inputs needed by a Java-only request.
    ///
    /// Keeping this check at the protocol boundary prevents Java syntax-tree
    /// providers from interpreting another language's node kinds while still
    /// letting Java providers remain focused on their own behavior.
    pub(crate) fn java_document(&self, uri: &Url, feature: &str) -> Option<(FileId, Arc<String>)> {
        let state = self.documents.by_uri.get(uri.as_str()).or_else(|| {
            warn!("{feature} requested for unopened document: {uri}");
            None
        })?;

        if state.language != Language::Java {
            debug!("{feature} is not supported for {}", state.language.name());
            return None;
        }
        if state.desynchronized {
            debug!(
                "{feature} deferred until {} receives a full text update",
                uri
            );
            return None;
        }

        Some((state.file_id, Arc::clone(&state.text)))
    }
}

impl Backend {
    /// Creates a new `Backend` instance.
    ///
    /// # Arguments
    ///
    /// * `client` - A `Client` handle for communicating with the LSP client
    ///
    /// # Returns
    ///
    /// A new `Backend` with empty document state and a fresh semantic database
    pub fn new(client: Client) -> Self {
        let project_classpaths = crate::project_classpath::ProjectClasspathStore::new();
        let workspace_root = Arc::new(RwLock::new(None));

        // Create empty workspace index (will be updated when workspace root is set)
        let workspace_index = Arc::new(WorkspaceIndex::new(PathBuf::from(".")));
        let host = SemanticHost::new(Arc::clone(&workspace_index));
        // Unit tests inside this crate prepare index state directly; every
        // integration and end-to-end test goes through the enforced path.
        #[cfg(not(test))]
        host.enforce_exclusive_index_writes();

        // Share the same workspace index with query engine and other providers
        // Note: QueryEngine requires owned WorkspaceIndex, so we clone it here
        // This is acceptable since WorkspaceIndex uses Arc<RwLock> internally
        let query_engine = Arc::new(QueryEngine::new((*workspace_index).clone()));

        let completion_provider = CompletionProvider::new(Arc::clone(&workspace_index));

        // Initialize source providers for JDK/external navigation (BEFORE the providers that use them)
        let source_providers = Arc::new(crate::source_provider::SourceProviderChain::new());
        let type_hierarchy_provider = crate::type_hierarchy::TypeHierarchyProvider::new(
            Arc::clone(&workspace_index),
            Arc::clone(&source_providers),
        );

        let symbols_provider = SymbolsProvider::new();

        let rename_provider = crate::rename::RenameProvider::new(
            Arc::clone(&workspace_index),
            Arc::clone(&query_engine),
            Arc::clone(&source_providers),
        );

        let semantic_tokens_provider = crate::semantic_tokens::SemanticTokensProvider::new();

        let folding_provider = crate::folding::FoldingProvider::new();

        let code_lens_provider = crate::code_lens::CodeLensProvider::new();

        let document_links_provider = crate::document_links::DocumentLinksProvider::new();

        let security_provider = Arc::new(
            crate::security_diagnostics::SecurityDiagnosticsProvider::with_project_classpaths(
                project_classpaths.clone(),
                Arc::clone(&workspace_root),
            ),
        );

        Self {
            client,
            documents: DocumentStore::new(),
            workspace: WorkspaceAnalysis {
                host,
                index: workspace_index,
                query: query_engine,
                project_classpaths,
                root: workspace_root,
                indexed_repos: Arc::new(DashMap::new()),
                source_root_gates: DashMap::new(),
                ready: Arc::new(AtomicBool::new(false)),
                destructive_refactoring: Arc::new(DestructiveRefactoringReadiness::new()),
                diagnostic_options: Arc::new(RwLock::new(
                    crate::diagnostic_engine::DiagnosticOptions::default(),
                )),
                file_events: Arc::new(std::sync::OnceLock::new()),
            },
            services: LanguageServices {
                completion: completion_provider,
                type_hierarchy: type_hierarchy_provider,
                symbols: symbols_provider,
                source_providers,
                rename: rename_provider,
                semantic_tokens: semantic_tokens_provider,
                folding: folding_provider,
                code_lens: code_lens_provider,
                document_links: document_links_provider,
                security: security_provider,
            },
            background_tasks: BackgroundTasks::new(),
            client_supports_progress: AtomicBool::new(false),
            client_supports_dynamic_type_hierarchy: AtomicBool::new(false),
            client_supports_dynamic_watchers: AtomicBool::new(false),
            client_workspace_edit_capabilities: std::sync::atomic::AtomicU8::new(0),
        }
    }
}

pub(crate) async fn parse_and_publish_diagnostics_for_uri(
    uri: &str,
    document_map: &Arc<DashMap<String, DocumentState>>,
    host: &Arc<SemanticHost>,
    client: &Client,
    security_provider: Option<&Arc<crate::security_diagnostics::SecurityDiagnosticsProvider>>,
    diagnostic_options: crate::diagnostic_engine::DiagnosticOptions,
    workspace_ready: Option<&Arc<AtomicBool>>,
    diagnostic_publication: &Arc<DiagnosticPublicationState>,
    expected_generation: u64,
    expected_workspace_revision: WorkspaceRevision,
) {
    let (file_id, source_text, language_id, version, change_version) = {
        let Some(doc_state) = document_map.get(uri) else {
            warn!("Attempted to parse unknown document: {}", uri);
            return;
        };
        (
            doc_state.file_id,
            doc_state.text.clone(),
            doc_state.language_id.clone(),
            doc_state.version,
            doc_state.change_version,
        )
    };

    let workspace_index = host.index();
    if workspace_index.current_workspace_revision() != expected_workspace_revision {
        debug!("Diagnostics superseded before analysis began: {uri}");
        return;
    }
    let Some(ticket) = diagnostic_publication.begin_analysis(
        uri,
        expected_generation,
        change_version,
        expected_workspace_revision,
        file_id,
    ) else {
        return;
    };

    let uri_parsed = match Url::parse(uri) {
        Ok(u) => u,
        Err(e) => {
            warn!("Failed to parse URI '{}': {}", uri, e);
            return;
        }
    };

    let is_workspace_ready = workspace_ready.is_none_or(|ready| ready.load(Ordering::SeqCst));

    // Phased publishing: each publish replaces the previous set for this URI.
    // Phase 1 (~1ms) delivers parse errors instantly, then phases 2-3 follow.
    let document_map_for_validation = Arc::clone(document_map);
    let workspace_index_for_validation = Arc::clone(workspace_index);
    let diagnostic_publication_for_validation = Arc::clone(diagnostic_publication);
    let uri_for_validation = uri.to_owned();
    let is_current = move || {
        diagnostic_analysis_is_current(
            &diagnostic_publication_for_validation,
            &uri_for_validation,
            ticket,
            &document_map_for_validation,
            &workspace_index_for_validation,
        )
    };

    let uri_for_publish = uri_parsed;
    let uri_text_for_publish = uri.to_owned();
    let version_for_publish = version;
    let document_map_for_publish = Arc::clone(document_map);
    let workspace_index_for_publish = Arc::clone(workspace_index);
    let diagnostic_publication_for_publish = Arc::clone(diagnostic_publication);
    let client_for_publish = client.clone();
    let publish_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    crate::diagnostic_engine::collect_and_publish_phased(
        file_id,
        &language_id,
        &source_text,
        host,
        security_provider,
        is_workspace_ready,
        diagnostic_options,
        is_current,
        move |diagnostics| {
            let uri = uri_for_publish.clone();
            let uri_text = uri_text_for_publish.clone();
            let document_map = Arc::clone(&document_map_for_publish);
            let workspace_index = Arc::clone(&workspace_index_for_publish);
            let diagnostic_publication = Arc::clone(&diagnostic_publication_for_publish);
            let publication = diagnostic_publication.uri_publication(&uri_text);
            let client = client_for_publish.clone();
            let phase = publish_count.fetch_add(1, Ordering::SeqCst);
            async move {
                let _publication_guard = publication.publication.lock().await;
                if !diagnostic_analysis_is_current(
                    &diagnostic_publication,
                    &uri_text,
                    ticket,
                    &document_map,
                    &workspace_index,
                ) {
                    return false;
                }
                // Publish stable results: the parse-only first
                // phase of a re-analysis must not wipe the semantic set the
                // client already shows, and an unchanged set is not re-sent.
                {
                    let mut published = publication.published.lock();
                    let parse_errors = if phase == 0 {
                        diagnostics.len()
                    } else {
                        published.as_ref().map_or(0, |p| p.parse_errors)
                    };
                    if let Some(last) = published.as_ref() {
                        let holds_back_semantic_set = phase == 0
                            && diagnostics.is_empty()
                            && last.parse_errors == 0
                            && !last.diagnostics.is_empty();
                        if holds_back_semantic_set || last.diagnostics == diagnostics {
                            return true;
                        }
                    }
                    *published = Some(state::PublishedDiagnostics {
                        diagnostics: diagnostics.clone(),
                        parse_errors,
                    });
                }
                client
                    .publish_diagnostics(uri, diagnostics, Some(version_for_publish))
                    .await;
                true
            }
        },
    )
    .await;
}

fn diagnostic_analysis_is_current(
    diagnostic_publication: &DiagnosticPublicationState,
    uri: &str,
    ticket: DiagnosticAnalysisTicket,
    document_map: &DashMap<String, DocumentState>,
    workspace_index: &WorkspaceIndex,
) -> bool {
    document_map.get(uri).is_some_and(|document| {
        diagnostic_publication.is_current_for(
            uri,
            ticket,
            document.file_id,
            document.change_version,
            workspace_index.current_workspace_revision(),
        )
    })
}

// ================================================================================================
// LANGUAGE SERVER PROTOCOL IMPLEMENTATION
// ================================================================================================
//
// This impl block contains all LSP request and notification handlers.
// The methods are organized into logical sections for better readability:
//
// 1. LIFECYCLE METHODS (initialize, initialized, shutdown)
// 2. NOTIFICATION HANDLERS (didOpen, didChange, didClose, willSave)
// 3. REQUEST HANDLERS (completion, hover, goto-definition, references, etc.)
//
// Each section is clearly marked with a comment banner.
//
// Long-running request cancellation pattern:
// 1. Capture `tower_lsp::CancellationToken::current()` at the handler boundary.
// 2. Call `check()` before and after every await.
// 3. Pass the token into synchronous provider code and inspect `is_cancelled()` at natural
//    loop/phase boundaries.
// 4. Call `check()` once more before returning so partial provider results never escape as a
//    successful response.
//
// Completion, references, rename, and workspace symbols are the reference implementations.

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    // ============================================================================================
    // LIFECYCLE METHODS
    // ============================================================================================

    /// Handles the `initialize` request from the client.
    ///
    /// This is the first request sent by the client after establishing a connection.
    /// The server responds with its capabilities, which inform the client about
    /// which LSP features are supported.
    ///
    /// # Capabilities
    ///
    /// Currently, the server declares:
    /// - Full text document synchronization with incremental updates
    /// - UTF-16 position encoding (LSP standard)
    ///
    /// # Arguments
    ///
    /// * `params` - Initialization parameters from the client, including client capabilities
    ///
    /// # Returns
    ///
    /// An `InitializeResult` containing the server's capabilities
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Running the project's build tool executes repository code, so the
        // client (never the repository) must opt in.
        if let Some(enabled) = params
            .initialization_options
            .as_ref()
            .and_then(crate::build_tool_classpath::setting_from_json)
        {
            crate::build_tool_classpath::set_enabled(enabled);
        }
        let workspace_capabilities = params.capabilities.workspace.as_ref();
        let supports_apply_edit =
            workspace_capabilities.and_then(|caps| caps.apply_edit) == Some(true);
        let supports_document_changes = workspace_capabilities
            .and_then(|caps| caps.workspace_edit.as_ref())
            .and_then(|caps| caps.document_changes)
            == Some(true);
        let supports_rename_resource_operation = workspace_capabilities
            .and_then(|caps| caps.workspace_edit.as_ref())
            .and_then(|caps| caps.resource_operations.as_ref())
            .is_some_and(|operations| operations.contains(&ResourceOperationKind::Rename));
        let resource_operation = |kind: ResourceOperationKind| {
            workspace_capabilities
                .and_then(|caps| caps.workspace_edit.as_ref())
                .and_then(|caps| caps.resource_operations.as_ref())
                .is_some_and(|operations| operations.contains(&kind))
        };
        let supports_create_resource_operation = resource_operation(ResourceOperationKind::Create);
        let supports_delete_resource_operation = resource_operation(ResourceOperationKind::Delete);
        self.client_workspace_edit_capabilities.store(
            u8::from(supports_apply_edit)
                | (u8::from(supports_document_changes) << 1)
                | (u8::from(supports_rename_resource_operation) << 2)
                | (u8::from(supports_create_resource_operation) << 3)
                | (u8::from(supports_delete_resource_operation) << 4),
            Ordering::Release,
        );
        self.services.completion.set_snippet_support(
            params
                .capabilities
                .text_document
                .as_ref()
                .and_then(|text_document| text_document.completion.as_ref())
                .and_then(|completion| completion.completion_item.as_ref())
                .and_then(|item| item.snippet_support)
                == Some(true),
        );
        let supports_dynamic_type_hierarchy = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|capabilities| capabilities.type_hierarchy.as_ref())
            .and_then(|capabilities| capabilities.dynamic_registration)
            == Some(true);
        let supports_code_action_literals = params
            .capabilities
            .text_document
            .as_ref()
            .and_then(|capabilities| capabilities.code_action.as_ref())
            .and_then(|capabilities| capabilities.code_action_literal_support.as_ref())
            .is_some();
        self.client_supports_dynamic_type_hierarchy
            .store(supports_dynamic_type_hierarchy, Ordering::SeqCst);
        self.client_supports_dynamic_watchers.store(
            workspace_capabilities
                .and_then(|caps| caps.did_change_watched_files.as_ref())
                .and_then(|caps| caps.dynamic_registration)
                == Some(true),
            Ordering::SeqCst,
        );

        // Prefer workspace_folders (modern), fall back to root_uri (deprecated)
        let root_path = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .and_then(|f| f.uri.to_file_path().ok())
            .or_else(|| {
                #[allow(deprecated)]
                params.root_uri.as_ref().and_then(|u| u.to_file_path().ok())
            });

        if let Some(mut root) = root_path {
            info!("Client-provided workspace root: {:?}", root);

            // For multi-module Gradle projects, find the true root by looking for settings.gradle
            // This handles cases where the editor opens a subproject but we need the monorepo root
            root = find_gradle_root(&root);

            info!("Initializing Hyperion LSP Server for workspace: {:?}", root);
            *self.workspace.root.write() = Some(root.clone());
            // Update the workspace index root path
            // This also updates the query engine since they share the same index
            self.workspace
                .host
                .write()
                .await
                .index()
                .set_root(root.clone());
            info!("Workspace root set to: {:?}", root);

            match crate::config::load_config(&root, None) {
                Ok(config) => {
                    let mut options = self.workspace.diagnostic_options.write();
                    *options = config.diagnostic_options();
                }
                Err(err) => {
                    warn!("Failed to load workspace config: {}", err);
                }
            }
        } else {
            info!("Initializing Hyperion LSP Server with no workspace root");
        }

        // Log client information for debugging
        if let Some(client_info) = params.client_info {
            info!(
                "Client: {} version {}",
                client_info.name,
                client_info.version.unwrap_or_else(|| "unknown".to_string())
            );
        }

        // Check if client supports workDoneProgress notifications
        if let Some(window_caps) = params.capabilities.window {
            if window_caps.work_done_progress == Some(true) {
                self.client_supports_progress.store(true, Ordering::SeqCst);
                info!("Client supports workDoneProgress notifications");
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // Text document synchronization capabilities
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::INCREMENTAL),
                        will_save: Some(false),
                        will_save_wait_until: Some(true),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(false),
                        })),
                    },
                )),

                // Position encoding (UTF-16 is the LSP standard)
                position_encoding: Some(PositionEncodingKind::UTF16),

                // Code completion
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string(), "@".to_string()]),
                    all_commit_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                    completion_item: None,
                }),

                // Hover information
                hover_provider: Some(HoverProviderCapability::Simple(true)),

                // Signature help (parameter hints)
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: None,
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),

                // lsp-types 0.95 has no static type_hierarchy_provider field.
                // Supporting clients are registered dynamically in initialized().

                // Navigation features
                definition_provider: Some(OneOf::Left(true)),
                implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
                references_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),

                // Workspace symbol search (Ctrl+T)
                workspace_symbol_provider: Some(OneOf::Left(true)),

                // Rename support
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                })),

                // Code actions (quick fixes and refactorings)
                code_action_provider: supports_code_action_literals.then_some(
                    CodeActionProviderCapability::Options(CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR,
                            CodeActionKind::REFACTOR_EXTRACT,
                            CodeActionKind::SOURCE,
                            CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
                        ]),
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                        resolve_provider: Some(false),
                    }),
                ),

                // Inlay hints (type and parameter hints)
                inlay_hint_provider: Some(OneOf::Left(true)),

                // Code formatting
                document_formatting_provider: Some(OneOf::Left(true)),
                document_range_formatting_provider: Some(OneOf::Left(true)),

                // Semantic tokens (semantic syntax highlighting)
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions::default(),
                            legend: crate::semantic_tokens::SemanticTokensProvider::legend(),
                            range: Some(false), // We don't support range yet
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),

                // Folding ranges
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),

                // Code lens (test runner, implementation counts)
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),

                // Document links (clickable URLs in comments)
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),

                // Execute command (code lens actions, command palette)
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: supported_commands(),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),

                // Call hierarchy (incoming/outgoing calls)
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),

                // Workspace capabilities for file watching
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: Some(file_operation_capabilities()),
                }),

                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "hyperion-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    /// Handles the `initialized` notification from the client.
    ///
    /// This notification is sent after the client receives the `initialize` response.
    /// At this point, the server is fully initialized and can start accepting requests.
    ///
    /// # Arguments
    ///
    /// * `_params` - Initialized parameters (currently unused)
    async fn initialized(&self, _params: InitializedParams) {
        if self
            .client_supports_dynamic_type_hierarchy
            .swap(false, Ordering::SeqCst)
        {
            let client = self.client.clone();
            tokio::spawn(async move {
                let registration = Registration {
                    id: "hyperion-type-hierarchy".to_string(),
                    method: "textDocument/prepareTypeHierarchy".to_string(),
                    register_options: Some(serde_json::json!({
                        "documentSelector": [
                            { "language": "java" },
                            { "language": "kotlin" }
                        ]
                    })),
                };
                if let Err(error) = client.register_capability(vec![registration]).await {
                    warn!("Failed to register type hierarchy capability: {error}");
                }
            });
        }

        if self
            .client_supports_dynamic_watchers
            .swap(false, Ordering::SeqCst)
        {
            let client = self.client.clone();
            tokio::spawn(async move {
                let globs = [
                    "**/*.java",
                    "**/*.kt",
                    "**/*.kts",
                    "**/pom.xml",
                    "**/*.gradle",
                    "**/*.gradle.kts",
                    "**/gradle.properties",
                    "**/libs.versions.toml",
                ];
                let registration = Registration {
                    id: "hyperion-watched-files".to_string(),
                    method: "workspace/didChangeWatchedFiles".to_string(),
                    register_options: Some(serde_json::json!({
                        "watchers": globs
                            .iter()
                            .map(|glob| serde_json::json!({ "globPattern": glob }))
                            .collect::<Vec<_>>()
                    })),
                };
                if let Err(error) = client.register_capability(vec![registration]).await {
                    warn!("Failed to register file watchers: {error}");
                }
            });
        }

        // Extract values from RwLock-guarded fields before the async call to avoid
        // holding non-Send guards across await points.
        let workspace_root = self.workspace.root.read().clone();
        crate::handlers::lifecycle::handle_initialized(
            workspace_root,
            self.client.clone(),
            self.client_supports_progress.load(Ordering::SeqCst),
            Arc::clone(&self.workspace.ready),
            Arc::clone(&self.documents.file_ids),
            self.documents.by_uri.clone(),
            Arc::clone(&self.documents.open_by_id),
            Arc::clone(self.services.completion.var_type_cache()),
            self.background_tasks.build_refresh.clone(),
            Arc::clone(&self.services.source_providers),
            self.invalidation_runtime(),
            Arc::clone(&self.workspace.file_events),
        )
        .await;
    }

    /// Handles the `shutdown` request from the client.
    ///
    /// This request is sent by the client to ask the server to shut down gracefully.
    /// The server should release resources and prepare to exit, but should not exit
    /// until it receives the `exit` notification.
    ///
    /// # Returns
    ///
    /// An empty result indicating successful shutdown preparation
    async fn shutdown(&self) -> Result<()> {
        info!("Hyperion LSP Server shutting down");
        self.background_tasks
            .diagnostic_publication
            .shutdown()
            .await;
        crate::handlers::lifecycle::handle_shutdown(
            self.workspace.root.read().clone(),
            &self.workspace.index,
            &self.documents.by_uri,
        );
        Ok(())
    }

    // ============================================================================================
    // NOTIFICATION HANDLERS
    // ============================================================================================

    async fn did_change_configuration(
        &self,
        params: tower_lsp::lsp_types::DidChangeConfigurationParams,
    ) {
        let Some(enabled) = crate::build_tool_classpath::setting_from_json(&params.settings) else {
            return;
        };
        let was_enabled = crate::build_tool_classpath::set_enabled(enabled);
        if enabled != was_enabled {
            info!("hyperion.buildToolClasspath changed to {enabled}");
            crate::build_tool_classpath::request_rerun();
            self.reload_project("hyperion.buildToolClasspath changed");
        }
    }

    async fn did_change_watched_files(
        &self,
        params: tower_lsp::lsp_types::DidChangeWatchedFilesParams,
    ) {
        self.handle_did_change_watched_files(params).await;
    }

    async fn did_create_files(&self, params: tower_lsp::lsp_types::CreateFilesParams) {
        self.handle_did_create_files(params).await;
    }

    async fn did_delete_files(&self, params: tower_lsp::lsp_types::DeleteFilesParams) {
        self.handle_did_delete_files(params).await;
    }

    async fn did_rename_files(&self, params: tower_lsp::lsp_types::RenameFilesParams) {
        self.handle_did_rename_files(params).await;
    }

    async fn will_rename_files(
        &self,
        params: tower_lsp::lsp_types::RenameFilesParams,
    ) -> Result<Option<WorkspaceEdit>> {
        self.handle_will_rename_files(params).await
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.handle_did_open(params).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.handle_did_change(params).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.handle_did_close(params).await;
    }

    async fn will_save_wait_until(
        &self,
        params: WillSaveTextDocumentParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        Ok(self.handle_will_save_wait_until(&params).await)
    }

    // ============================================================================================
    // REQUEST HANDLERS - Code Intelligence
    // ============================================================================================

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        self.handle_completion(params).await
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        Ok(self.handle_hover(params).await)
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        Ok(self.handle_signature_help(params).await)
    }

    async fn inlay_hint(
        &self,
        params: tower_lsp::lsp_types::InlayHintParams,
    ) -> Result<Option<Vec<InlayHint>>> {
        Ok(self.handle_inlay_hint(params).await)
    }

    // ============================================================================================
    // REQUEST HANDLERS - Type & Call Hierarchy
    // ============================================================================================

    async fn prepare_type_hierarchy(
        &self,
        params: TypeHierarchyPrepareParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        Ok(self.handle_prepare_type_hierarchy(params).await)
    }

    async fn supertypes(
        &self,
        params: TypeHierarchySupertypesParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        Ok(self.handle_supertypes(params).await)
    }

    async fn subtypes(
        &self,
        params: TypeHierarchySubtypesParams,
    ) -> Result<Option<Vec<TypeHierarchyItem>>> {
        Ok(self.handle_subtypes(params).await)
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        Ok(self.handle_prepare_call_hierarchy(params).await)
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        Ok(self.handle_incoming_calls(params).await)
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        Ok(self.handle_outgoing_calls(params).await)
    }

    // ============================================================================================
    // REQUEST HANDLERS - Navigation
    // ============================================================================================

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(self.handle_goto_definition(params).await)
    }

    async fn goto_implementation(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(self.handle_goto_implementation(params).await)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        self.handle_references(params).await
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        Ok(self.handle_document_highlight(params).await)
    }

    // ============================================================================================
    // REQUEST HANDLERS - Symbols
    // ============================================================================================

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        Ok(self.handle_document_symbol(params).await)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        self.handle_workspace_symbol(params)
    }

    // ============================================================================================
    // REQUEST HANDLERS - Refactoring
    // ============================================================================================

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        Ok(self.handle_prepare_rename(params))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        self.handle_rename(params).await
    }

    /// Every action already carries its edit or command, so resolving one is
    /// the identity. Some clients resolve actions without an `edit` (such as
    /// command actions) regardless of `resolveProvider`; answering
    /// method-not-found leaves them waiting.
    async fn code_action_resolve(
        &self,
        action: tower_lsp::lsp_types::CodeAction,
    ) -> Result<tower_lsp::lsp_types::CodeAction> {
        Ok(action)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        Ok(self.handle_code_action(params).await)
    }

    // ============================================================================================
    // REQUEST HANDLERS - Formatting
    // ============================================================================================

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        Ok(self.handle_formatting(params).await)
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        Ok(self.handle_range_formatting(params))
    }

    // ============================================================================================
    // REQUEST HANDLERS - Misc
    // ============================================================================================

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        Ok(self.handle_semantic_tokens_full(params).await)
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        Ok(self.handle_folding_range(params).await)
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        Ok(self.handle_document_link(params).await)
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        Ok(self.handle_code_lens(params).await)
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        Ok(self.handle_execute_command(params).await)
    }
}

fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut cur = if path.is_file() {
        path.parent()?.to_path_buf()
    } else {
        path.to_path_buf()
    };

    loop {
        let dot_git = cur.join(".git");
        if dot_git.is_dir() || dot_git.is_file() {
            return Some(cur);
        }

        if !cur.pop() {
            break;
        }
    }

    None
}

pub(crate) fn find_nearest_source_root(file_path: &Path) -> Option<PathBuf> {
    // Look for .../src/{main,test}/{java,kotlin,scala}/... patterns and return the
    // directory at that language root (e.g. .../src/main/java).
    for ancestor in file_path.ancestors() {
        let Some(lang) = ancestor.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !matches!(lang, "java" | "kotlin" | "scala") {
            continue;
        }

        let parent = ancestor.parent()?;
        let Some(set) = parent.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !matches!(set, "main" | "test") {
            continue;
        }

        let grand = parent.parent()?;
        if grand.file_name().and_then(|n| n.to_str()) != Some("src") {
            continue;
        }

        return Some(ancestor.to_path_buf());
    }

    None
}

// Re-export build system utilities for backwards compatibility.
// These functions are now defined in `handlers::build_system`.
pub use crate::handlers::build_system::find_gradle_root;
pub(crate) use crate::handlers::build_system::schedule_build_system_refresh;

/// Creates and returns the LSP service.
///
/// This function sets up the LSP service with the Backend.
///
/// # Returns
///
/// A tuple containing the `LspService` and the `ClientSocket`
pub fn create_lsp_server() -> (LspService<Backend>, tower_lsp::ClientSocket) {
    LspService::new(Backend::new)
}

/// File operations the server wants to hear about: Java/Kotlin files and any
/// folder (a moved directory moves every source below it).
fn file_operation_capabilities() -> tower_lsp::lsp_types::WorkspaceFileOperationsServerCapabilities
{
    use tower_lsp::lsp_types::{
        FileOperationFilter, FileOperationPattern, FileOperationPatternKind,
        FileOperationRegistrationOptions,
    };
    let options = || FileOperationRegistrationOptions {
        filters: vec![
            FileOperationFilter {
                scheme: Some("file".to_string()),
                pattern: FileOperationPattern {
                    glob: "**/*.{java,kt}".to_string(),
                    matches: Some(FileOperationPatternKind::File),
                    options: None,
                },
            },
            FileOperationFilter {
                scheme: Some("file".to_string()),
                pattern: FileOperationPattern {
                    glob: "**".to_string(),
                    matches: Some(FileOperationPatternKind::Folder),
                    options: None,
                },
            },
        ],
    };
    tower_lsp::lsp_types::WorkspaceFileOperationsServerCapabilities {
        did_create: Some(options()),
        will_create: None,
        did_rename: Some(options()),
        will_rename: Some(options()),
        did_delete: Some(options()),
        will_delete: None,
    }
}
