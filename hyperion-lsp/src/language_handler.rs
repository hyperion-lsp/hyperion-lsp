// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Unified language dispatch for LSP providers.
//!
//! Instead of duplicating 15-25 line match blocks in hover, navigation, and find-references,
//! each non-Java language implements `LanguageHandler`. Providers call `handler_for(language)`
//! once — Java returns `None` and falls through to the inline implementation.
//!
//! # Adding a capability
//!
//! Capabilities come in two shapes:
//!
//! * **Mandatory** (`hover`, `goto_definition`): every non-Java language must
//!   implement it, so the method has no default.
//! * **Opt-in** (`find_references`, `document_symbols`, `signature_help`,
//!   `inlay_hints`, and everything added from here on): the method returns
//!   `Option<Result<T, E>>` and defaults to `None`, which means *"this handler
//!   declines the request"*. The provider must then apply its explicit
//!   language policy; `None` is not permission to parse a foreign CST as Java.
//!
//! Slotting in a new opt-in capability (completion, code lens, …) is
//! therefore purely additive: add a defaulted `fn` here, override
//! it in the handlers that support it, and have the provider do
//!
//! ```ignore
//! if let Some(result) = ctx.handler_for(language).and_then(|h| h.new_capability(..)) {
//!     return result;
//! }
//! ```
//!
//! Existing handlers need not change, but the call site must define the safe
//! behavior for a language that declines the capability.

use crate::find_references_kotlin::kotlin_find_references;
use crate::groovy_support;
use crate::hover::gradle::GradleHoverHelper;
use crate::hover::groovy::GroovyHoverHelper;
use crate::hover::kotlin::KotlinHoverHelper;
use crate::hover::scala::ScalaHoverHelper;
use crate::navigation::groovy::GroovyNavigationHelper;
use crate::navigation::kotlin::KotlinNavigationHelper;
use crate::navigation::scala::ScalaNavigationHelper;
use crate::semantic_host::ReadTxn;
use crate::source_provider::SourceProviderChain;
use hyperion_semantics::db::{FileId, ParseDatabase};
use hyperion_workspace::{Language, QueryEngine, WorkspaceIndex};
use std::sync::Arc;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, DocumentSymbol, Hover,
    InlayHint, Location, Position, Range, SignatureHelp, TypeHierarchyItem, Url,
};
use tracing::debug;

/// Trait for language-specific LSP operations.
///
/// Each non-Java language implements this to provide hover and goto-definition,
/// plus any of the opt-in capabilities it supports. Java has no handler and is
/// implemented inline by providers. See the module docs for how to add a new
/// capability without weakening the provider's language boundary.
pub(crate) trait LanguageHandler {
    fn hover(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Hover>, String>;

    fn goto_definition(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Location>, String>;

    /// Returns `Some` to handle references or `None` to decline the request.
    fn find_references(
        &self,
        _file_id: FileId,
        _position: Position,
        _source_text: &str,
        _uri: &Url,
        _include_declaration: bool,
    ) -> Option<Result<Vec<Location>, String>> {
        None
    }

    /// Resolve the identifier token under a rename cursor.
    ///
    /// Returning `None` delegates the decision to the caller; it must not decode
    /// a foreign green tree through Java's `SyntaxKind` values.
    fn rename_token(
        &self,
        _file_id: FileId,
        _position: Position,
        _source_text: &str,
    ) -> Option<Result<Option<crate::rename::RenameToken>, String>> {
        None
    }

    /// Document outline (`textDocument/documentSymbol`).
    ///
    /// Returns `Some` to handle the request or `None` to decline it. Languages
    /// with their own CST (Kotlin) *must* override this: the
    /// Java walker in [`crate::symbols`] decodes rowan kinds through
    /// `hyperion_parser::SyntaxKind` and would misread a foreign tree.
    fn document_symbols(
        &self,
        _file_id: FileId,
        _source_text: &str,
    ) -> Option<Result<Vec<DocumentSymbol>, String>> {
        None
    }

    /// Parameter hints for a call being typed (`textDocument/signatureHelp`).
    ///
    /// Returns `Some` to handle the request or `None` to decline it. As with
    /// [`Self::document_symbols`], a language with its own CST
    /// *must* override this rather than let the Java walker decode a foreign
    /// green tree through `hyperion_parser::SyntaxKind`.
    fn signature_help(
        &self,
        _file_id: FileId,
        _position: Position,
        _source_text: &str,
    ) -> Option<Result<Option<SignatureHelp>, String>> {
        None
    }

    /// Inline type and parameter hints (`textDocument/inlayHint`).
    ///
    /// Returns `Some` to handle the request or `None` to decline it.
    fn inlay_hints(
        &self,
        _file_id: FileId,
        _range: Range,
        _source_text: &str,
    ) -> Option<Result<Vec<InlayHint>, String>> {
        None
    }

    /// The callable under the cursor (`textDocument/prepareCallHierarchy`).
    ///
    /// Returns `Some` to handle the request or `None` to decline it. As with
    /// [`Self::document_symbols`], a language with its own CST
    /// *must* override this rather than let the Java walker decode a foreign
    /// green tree through `hyperion_parser::SyntaxKind`.
    fn prepare_call_hierarchy(
        &self,
        _file_id: FileId,
        _position: Position,
        _source_text: &str,
        _uri: &Url,
    ) -> Option<Result<Option<Vec<CallHierarchyItem>>, String>> {
        None
    }

    /// Callers of a hierarchy item (`callHierarchy/incomingCalls`).
    ///
    /// Dispatched on the *item's* language, which is the language of the callee
    /// — the callers themselves may be written in anything.
    fn incoming_calls(
        &self,
        _item: &CallHierarchyItem,
    ) -> Option<Result<Vec<CallHierarchyIncomingCall>, String>> {
        None
    }

    /// Calls made from a hierarchy item (`callHierarchy/outgoingCalls`).
    fn outgoing_calls(
        &self,
        _item: &CallHierarchyItem,
    ) -> Option<Result<Vec<CallHierarchyOutgoingCall>, String>> {
        None
    }

    /// The type under the cursor (`textDocument/prepareTypeHierarchy`).
    fn prepare_type_hierarchy(
        &self,
        _file_id: FileId,
        _position: Position,
        _source_text: &str,
    ) -> Option<Result<Option<Vec<TypeHierarchyItem>>, String>> {
        None
    }

    /// Supertypes of a hierarchy item (`typeHierarchy/supertypes`).
    fn supertypes(
        &self,
        _item: &TypeHierarchyItem,
    ) -> Option<Result<Option<Vec<TypeHierarchyItem>>, String>> {
        None
    }

    /// Subtypes of a hierarchy item (`typeHierarchy/subtypes`).
    fn subtypes(
        &self,
        _item: &TypeHierarchyItem,
    ) -> Option<Result<Option<Vec<TypeHierarchyItem>>, String>> {
        None
    }

    /// Concrete implementations of the type under the cursor.
    ///
    /// This is separate from `typeHierarchy/subtypes`: goto-implementation is
    /// transitive and must not run Java's CST walker for another language.
    fn goto_implementation(
        &self,
        _file_id: FileId,
        _position: Position,
        _source_text: &str,
    ) -> Option<Result<Option<Vec<Location>>, String>> {
        None
    }
}

/// Shared references needed by all language handlers.
pub(crate) struct LanguageHandlerContext<'a> {
    pub txn: &'a ReadTxn,
    pub workspace_index: &'a Arc<WorkspaceIndex>,
    pub source_providers: &'a Arc<SourceProviderChain>,
    /// Only needed for find_references dispatch (hover/navigation don't use it).
    pub query_engine: Option<&'a Arc<QueryEngine>>,
}

impl LanguageHandlerContext<'_> {
    /// Returns `None` for Java, whose providers use their inline implementation.
    pub fn handler_for(&self, language: Language) -> Option<Box<dyn LanguageHandler + '_>> {
        match language {
            Language::Java => None,
            Language::Kotlin => Some(Box::new(KotlinHandler { ctx: self })),
            Language::Groovy => Some(Box::new(GroovyHandler { ctx: self })),
            Language::Scala => Some(Box::new(ScalaHandler { ctx: self })),
        }
    }

    /// Dispatch an opt-in capability without letting a foreign CST reach Java.
    ///
    /// Java deliberately has no [`LanguageHandler`], so only Java may invoke
    /// `java`. A non-Java handler that declines the capability invokes
    /// `declined` instead. Keeping that distinction here makes the safety
    /// boundary explicit at every opt-in request site.
    pub fn dispatch_opt_in<T>(
        &self,
        language: Language,
        non_java: impl FnOnce(&dyn LanguageHandler) -> Option<Result<T, String>>,
        java: impl FnOnce() -> Result<T, String>,
        declined: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        match self.handler_for(language) {
            Some(handler) => non_java(handler.as_ref()).unwrap_or_else(declined),
            None => java(),
        }
    }
}

// ---------------------------------------------------------------------------
// Kotlin
// ---------------------------------------------------------------------------

struct KotlinHandler<'a> {
    ctx: &'a LanguageHandlerContext<'a>,
}

impl LanguageHandler for KotlinHandler<'_> {
    fn hover(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Hover>, String> {
        debug!("HOVER: Kotlin file detected, using Kotlin hover");
        KotlinHoverHelper::new(
            self.ctx.txn,
            self.ctx.workspace_index,
            self.ctx.source_providers,
        )
        .hover_kotlin(file_id, position, source_text)
    }

    fn goto_definition(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Location>, String> {
        debug!("NAV: Kotlin file detected, using Kotlin goto-definition");
        KotlinNavigationHelper::new(
            self.ctx.txn,
            self.ctx.workspace_index,
            self.ctx.source_providers,
        )
        .goto_definition(file_id, position, source_text, uri)
    }

    fn find_references(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
        include_declaration: bool,
    ) -> Option<Result<Vec<Location>, String>> {
        let query_engine = self.ctx.query_engine?;
        debug!("REFS: Kotlin file detected, using Kotlin find-references");
        Some(kotlin_find_references(
            self.ctx.txn,
            self.ctx.workspace_index,
            query_engine,
            self.ctx.source_providers,
            file_id,
            position,
            source_text,
            uri,
            include_declaration,
        ))
    }

    fn rename_token(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Option<Result<Option<crate::rename::RenameToken>, String>> {
        Some(crate::rename::kotlin_rename_token(
            self.ctx.txn,
            file_id,
            position,
            source_text,
        ))
    }

    fn document_symbols(
        &self,
        file_id: FileId,
        source_text: &str,
    ) -> Option<Result<Vec<DocumentSymbol>, String>> {
        debug!("SYMBOLS: Kotlin file detected, using Kotlin document symbols");
        Some(kotlin_document_symbols_for_file(
            self.ctx.txn,
            file_id,
            source_text,
        ))
    }

    fn signature_help(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Option<Result<Option<SignatureHelp>, String>> {
        debug!("SIGHELP: Kotlin file detected, using Kotlin signature help");
        Some(
            crate::signature_help_kotlin::KotlinSignatureHelpProvider::new(
                self.ctx.txn,
                Arc::clone(self.ctx.workspace_index),
            )
            .signature_help(file_id, position, source_text),
        )
    }

    fn inlay_hints(
        &self,
        file_id: FileId,
        range: Range,
        source_text: &str,
    ) -> Option<Result<Vec<InlayHint>, String>> {
        debug!("INLAY: Kotlin file detected, using Kotlin inlay hints");
        Some(
            crate::inlay_hints_kotlin::KotlinInlayHintsProvider::new(
                self.ctx.txn,
                Arc::clone(self.ctx.workspace_index),
            )
            .inlay_hints(file_id, range, source_text),
        )
    }

    fn prepare_call_hierarchy(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Option<Result<Option<Vec<CallHierarchyItem>>, String>> {
        debug!("CALLHIER: Kotlin file detected, using Kotlin call hierarchy");
        Some(
            self.call_hierarchy()
                .prepare(file_id, position, source_text, uri),
        )
    }

    fn incoming_calls(
        &self,
        item: &CallHierarchyItem,
    ) -> Option<Result<Vec<CallHierarchyIncomingCall>, String>> {
        debug!("CALLHIER: Kotlin item, using Kotlin incoming calls");
        Some(self.call_hierarchy().incoming_calls(item))
    }

    fn outgoing_calls(
        &self,
        item: &CallHierarchyItem,
    ) -> Option<Result<Vec<CallHierarchyOutgoingCall>, String>> {
        debug!("CALLHIER: Kotlin item, using Kotlin outgoing calls");
        Some(self.call_hierarchy().outgoing_calls(item))
    }

    fn prepare_type_hierarchy(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Option<Result<Option<Vec<TypeHierarchyItem>>, String>> {
        debug!("TYPEHIER: Kotlin file detected, using Kotlin type hierarchy");
        Some(
            self.type_hierarchy()
                .prepare(file_id, position, source_text),
        )
    }

    fn supertypes(
        &self,
        item: &TypeHierarchyItem,
    ) -> Option<Result<Option<Vec<TypeHierarchyItem>>, String>> {
        debug!("TYPEHIER: Kotlin item, using Kotlin supertypes");
        Some(self.type_hierarchy().supertypes(item))
    }

    fn subtypes(
        &self,
        item: &TypeHierarchyItem,
    ) -> Option<Result<Option<Vec<TypeHierarchyItem>>, String>> {
        debug!("TYPEHIER: Kotlin item, using Kotlin subtypes");
        Some(self.type_hierarchy().subtypes(item))
    }

    fn goto_implementation(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Option<Result<Option<Vec<Location>>, String>> {
        debug!("TYPEHIER: Kotlin file detected, using Kotlin goto-implementation");
        Some(
            self.type_hierarchy()
                .implementation_locations(file_id, position, source_text),
        )
    }
}

impl KotlinHandler<'_> {
    fn call_hierarchy(&self) -> crate::call_hierarchy_kotlin::KotlinCallHierarchyProvider<'_> {
        crate::call_hierarchy_kotlin::KotlinCallHierarchyProvider::new(
            self.ctx.txn,
            Arc::clone(self.ctx.workspace_index),
        )
    }

    fn type_hierarchy(&self) -> crate::type_hierarchy_kotlin::KotlinTypeHierarchyProvider {
        crate::type_hierarchy_kotlin::KotlinTypeHierarchyProvider::new(
            Arc::clone(self.ctx.workspace_index),
            Arc::clone(self.ctx.source_providers),
        )
    }
}

/// Reinterpret the green tree with Kotlin's `SyntaxKind` and walk it.
fn kotlin_document_symbols_for_file(
    txn: &ReadTxn,
    file_id: FileId,
    source_text: &str,
) -> Result<Vec<DocumentSymbol>, String> {
    let parse_tree = txn.parse_file(file_id);

    let root = hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());
    Ok(crate::symbols_kotlin::kotlin_document_symbols(
        &root,
        source_text,
    ))
}

// ---------------------------------------------------------------------------
// Groovy (includes Gradle detection)
// ---------------------------------------------------------------------------

struct GroovyHandler<'a> {
    ctx: &'a LanguageHandlerContext<'a>,
}

impl LanguageHandler for GroovyHandler<'_> {
    fn hover(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Hover>, String> {
        if let Some(file_path) = self.ctx.workspace_index.get_file_path(file_id) {
            if groovy_support::is_gradle_file(&file_path.to_string_lossy()) {
                debug!("HOVER: Gradle file detected, using Gradle hover");
                return GradleHoverHelper::new().hover(position, source_text);
            }
        }
        debug!("HOVER: Groovy file detected, using Groovy hover");
        GroovyHoverHelper::new(self.ctx.workspace_index).hover(file_id, position, source_text)
    }

    fn goto_definition(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Location>, String> {
        let helper =
            GroovyNavigationHelper::new(self.ctx.workspace_index, self.ctx.source_providers);
        if let Some(file_path) = self.ctx.workspace_index.get_file_path(file_id) {
            if groovy_support::is_gradle_file(&file_path.to_string_lossy()) {
                debug!("NAV: Gradle file detected, using Gradle goto-definition");
                return helper.goto_gradle_definition(file_id, position, source_text, uri);
            }
        }
        debug!("NAV: Groovy file detected, using Groovy goto-definition");
        helper.goto_definition_groovy(file_id, position, source_text, uri)
    }

    // Opt-in Java-CST capabilities use their fail-closed defaults.
}

// ---------------------------------------------------------------------------
// Scala
// ---------------------------------------------------------------------------

struct ScalaHandler<'a> {
    ctx: &'a LanguageHandlerContext<'a>,
}

impl LanguageHandler for ScalaHandler<'_> {
    fn hover(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Hover>, String> {
        debug!("HOVER: Scala file detected, using Scala hover");
        ScalaHoverHelper::new(self.ctx.workspace_index).hover(file_id, position, source_text)
    }

    fn goto_definition(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Location>, String> {
        debug!("NAV: Scala file detected, using Scala goto-definition");
        ScalaNavigationHelper::new(self.ctx.workspace_index, self.ctx.source_providers)
            .goto_definition(file_id, position, source_text, uri)
    }

    // Opt-in Java-CST capabilities use their fail-closed defaults.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn declined_foreign_capability_does_not_invoke_java_fallback() {
        let workspace_index = Arc::new(WorkspaceIndex::new(std::env::temp_dir()));
        let host = crate::semantic_host::SemanticHost::new(Arc::clone(&workspace_index));
        let txn = host.read_now();
        let source_providers = Arc::new(SourceProviderChain::new());
        let ctx = LanguageHandlerContext {
            txn: &txn,
            workspace_index: &workspace_index,
            source_providers: &source_providers,
            query_engine: None,
        };

        for language in [Language::Groovy, Language::Scala] {
            let java_called = Cell::new(false);
            let result = ctx.dispatch_opt_in(
                language,
                |handler| {
                    handler.find_references(
                        FileId(0),
                        Position::new(0, 0),
                        "class JavaLooking {}",
                        &Url::parse("file:///JavaLooking.groovy").unwrap(),
                        true,
                    )
                },
                || {
                    java_called.set(true);
                    Ok(vec![])
                },
                || Ok(vec![]),
            );

            assert!(result.unwrap().is_empty());
            assert!(!java_called.get(), "{language:?} reached the Java fallback");
        }
    }

    #[test]
    fn java_capability_uses_java_fallback() {
        let workspace_index = Arc::new(WorkspaceIndex::new(std::env::temp_dir()));
        let host = crate::semantic_host::SemanticHost::new(Arc::clone(&workspace_index));
        let txn = host.read_now();
        let source_providers = Arc::new(SourceProviderChain::new());
        let ctx = LanguageHandlerContext {
            txn: &txn,
            workspace_index: &workspace_index,
            source_providers: &source_providers,
            query_engine: None,
        };
        let java_called = Cell::new(false);

        ctx.dispatch_opt_in(
            Language::Java,
            |_| panic!("Java must not have a foreign-language handler"),
            || {
                java_called.set(true);
                Ok(())
            },
            || panic!("Java must not take the declined-language branch"),
        )
        .unwrap();

        assert!(java_called.get());
    }
}
