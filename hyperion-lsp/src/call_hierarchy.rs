// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Call Hierarchy Provider
//!
//! Implements LSP call hierarchy for navigating method calls.
//! Supports "Incoming Calls" (who calls this method) and "Outgoing Calls" (what this method calls).

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_parser::{Lexer, Parser};
use hyperion_semantics::db::FileId;
use hyperion_workspace::{
    DeclarationNamespace, FileRevisionToken, IndexedContent, Language, QueryEngine, SymbolTarget,
    TextRange, WorkspaceIndex, WorkspaceRevision,
};
use std::collections::HashMap;
use std::sync::Arc;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Position, Range,
    SymbolKind, Url,
};
use tracing::debug;

use crate::navigation::import_resolver::extract_package_from_root;
use crate::navigation::NavigationProvider;
use crate::navigation_util::{offset_to_position, position_to_offset};
use crate::semantic_host::ReadTxn;

/// Provides call hierarchy information for methods
pub struct CallHierarchyProvider<'txn> {
    workspace_index: Arc<WorkspaceIndex>,
    navigation_provider: NavigationProvider<'txn>,
}

/// Immutable identity carried by every call-hierarchy item.
///
/// LSP `data` crosses JSON implementations whose numeric precision may stop at
/// 53 bits, so both monotonic `u64` revisions are encoded as decimal strings.
/// `file_id` remains numeric because its complete `u32` range is JSON-safe.
#[derive(Debug, Clone)]
pub(crate) struct CallHierarchyItemProvenance {
    pub fqn: String,
    pub file_revision: FileRevisionToken,
    pub workspace_revision: WorkspaceRevision,
}

pub(crate) struct ValidatedCallHierarchyItem {
    pub provenance: CallHierarchyItemProvenance,
    pub indexed: IndexedContent,
}

pub(crate) fn call_hierarchy_item_data(
    fqn: impl Into<String>,
    file_revision: FileRevisionToken,
    workspace_revision: WorkspaceRevision,
) -> serde_json::Value {
    serde_json::json!({
        "fqn": fqn.into(),
        "file_id": file_revision.file_id().as_u32(),
        "file_revision": file_revision.revision_id().get().to_string(),
        "workspace_revision": workspace_revision.get().get().to_string(),
    })
}

fn encoded_item_revision(item: &CallHierarchyItem, key: &str) -> Result<u64, String> {
    item.data
        .as_ref()
        .and_then(|data| data.get(key))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("Missing {key} in call hierarchy item data"))?
        .parse()
        .map_err(|_| format!("Invalid {key} in call hierarchy item data"))
}

fn encoded_item_file_id(item: &CallHierarchyItem) -> Result<FileId, String> {
    let raw = item
        .data
        .as_ref()
        .and_then(|data| data.get("file_id"))
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "Missing file_id in call hierarchy item data".to_string())?;
    let raw = u32::try_from(raw)
        .map_err(|_| "Invalid file_id in call hierarchy item data".to_string())?;
    Ok(FileId::new(raw))
}

pub(crate) fn item_fqn(item: &CallHierarchyItem) -> Result<String, String> {
    item.data
        .as_ref()
        .and_then(|data| data.get("fqn"))
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .ok_or_else(|| "Missing FQN in call hierarchy item data".to_string())
}

fn outgoing_target_identity(
    item: &CallHierarchyItem,
) -> Result<(String, DeclarationNamespace), String> {
    Ok((item_fqn(item)?, DeclarationNamespace::for_kind(item.kind)))
}

/// Validate an item and recover only the exact indexed bytes that own it.
pub(crate) fn validate_call_hierarchy_item(
    workspace_index: &WorkspaceIndex,
    item: &CallHierarchyItem,
) -> Result<ValidatedCallHierarchyItem, String> {
    let file_id = encoded_item_file_id(item)?;
    let expected_file_revision = encoded_item_revision(item, "file_revision")?;
    let expected_workspace_revision = encoded_item_revision(item, "workspace_revision")?;
    let workspace_revision = workspace_index.current_workspace_revision();
    if workspace_revision.get().get() != expected_workspace_revision {
        return Err("Stale call hierarchy item: workspace revision changed".to_string());
    }
    let file_revision = workspace_index
        .current_file_revision(file_id)
        .filter(|revision| revision.revision_id().get() == expected_file_revision)
        .ok_or_else(|| "Stale call hierarchy item: file revision changed".to_string())?;
    let indexed = workspace_index
        .get_indexed_revision_content(file_revision)
        .ok_or_else(|| "Exact call hierarchy source is unavailable".to_string())?;
    let indexed_uri = Url::from_file_path(&indexed.file_path)
        .map_err(|()| "Call hierarchy source path is not a file URI".to_string())?;
    if indexed_uri != item.uri {
        return Err("Stale call hierarchy item: source path changed".to_string());
    }
    if !workspace_index.workspace_revisions_are_current(workspace_revision, &[file_revision]) {
        return Err("Stale call hierarchy item: source changed during validation".to_string());
    }
    Ok(ValidatedCallHierarchyItem {
        provenance: CallHierarchyItemProvenance {
            fqn: item_fqn(item)?,
            file_revision,
            workspace_revision,
        },
        indexed,
    })
}

/// Bind supplied request bytes to one exact current index publication.
pub(crate) fn exact_call_hierarchy_source(
    workspace_index: &WorkspaceIndex,
    file_id: FileId,
    source: &str,
    uri: &Url,
) -> Result<(IndexedContent, WorkspaceRevision), String> {
    let workspace_revision = workspace_index.current_workspace_revision();
    let file_revision = workspace_index
        .current_file_revision(file_id)
        .ok_or_else(|| "Call hierarchy source is not indexed".to_string())?;
    let indexed = workspace_index
        .get_indexed_revision_content(file_revision)
        .ok_or_else(|| "Exact call hierarchy source is unavailable".to_string())?;
    let indexed_uri = Url::from_file_path(&indexed.file_path)
        .map_err(|()| "Call hierarchy source path is not a file URI".to_string())?;
    if indexed.content.as_str() != source || &indexed_uri != uri {
        return Err("Call hierarchy request bytes do not match the indexed revision".to_string());
    }
    if !workspace_index.workspace_revisions_are_current(workspace_revision, &[file_revision]) {
        return Err("Call hierarchy source changed during validation".to_string());
    }
    Ok((indexed, workspace_revision))
}

impl<'txn> CallHierarchyProvider<'txn> {
    /// Create a new CallHierarchyProvider
    pub fn new(txn: &'txn ReadTxn, workspace_index: Arc<WorkspaceIndex>) -> Self {
        let query_engine = Arc::new(QueryEngine::new(workspace_index.as_ref().clone()));
        let navigation_provider = NavigationProvider::new(
            txn,
            Arc::clone(&workspace_index),
            query_engine,
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        Self {
            workspace_index,
            navigation_provider,
        }
    }

    /// Prepare call hierarchy - find the method at the cursor position
    ///
    /// Returns a CallHierarchyItem for the method at cursor, or None if cursor
    /// is not on a method.
    pub fn prepare(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Vec<CallHierarchyItem>>, String> {
        debug!(
            "Preparing call hierarchy at position {:?} in file {:?}",
            position, file_id
        );

        let (indexed, workspace_revision) =
            exact_call_hierarchy_source(&self.workspace_index, file_id, source_text, uri)?;
        if Language::from_path(&indexed.file_path) != Some(Language::Java) {
            return Err("Java call hierarchy received a non-Java source".to_string());
        }

        // Convert position to offset
        let offset = position_to_offset(source_text, position)
            .map_err(|e| format!("Failed to convert position: {e}"))?;

        // Parse the exact request/index bytes directly. A Salsa snapshot may
        // contain a different publication for this FileId.
        let (green, errors) = Parser::parse(Lexer::new(source_text).collect());
        if !errors.is_empty() {
            debug!("Call hierarchy: Java source has parse errors");
            return Ok(None);
        }
        let root = SyntaxNode::new_root(green);

        // Find the token at the cursor
        let token = if let Some(t) = root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()
        {
            t
        } else {
            debug!("No token at cursor position");
            return Ok(None);
        };

        debug!("Token at cursor: '{}'", token.text());

        // Walk up to find METHOD_DECL or CONSTRUCTOR_DECL
        let mut node = match token.parent() {
            Some(n) => n,
            None => return Ok(None),
        };

        loop {
            match node.kind() {
                SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL => {
                    debug!("Found method declaration: {:?}", node.kind());

                    // Extract method name
                    let method_name_token = self.extract_method_name_token(&node)?;
                    let method_name = method_name_token.text().to_string();
                    debug!("Method name: {}", method_name);

                    let kind = if node.kind() == SyntaxKind::CONSTRUCTOR_DECL {
                        SymbolKind::CONSTRUCTOR
                    } else {
                        SymbolKind::METHOD
                    };

                    // Find containing class
                    let (class_name, _) = self.find_containing_class(&node)?;
                    let Some(method_fqn) = self.exact_java_declaration_fqn(
                        file_id,
                        &method_name,
                        method_name_token.text_range(),
                        indexed.revision,
                        workspace_revision,
                        DeclarationNamespace::for_kind(kind),
                    )?
                    else {
                        debug!("Call hierarchy: exact overload identity is unavailable");
                        return Ok(None);
                    };

                    // Get method range
                    let text_range = node.text_range();
                    let start_pos = offset_to_position(source_text, text_range.start().into());
                    let end_pos = offset_to_position(source_text, text_range.end().into());
                    let range = Range {
                        start: start_pos,
                        end: end_pos,
                    };

                    // Find just the method name for selection_range
                    let id_range = method_name_token.text_range();
                    let selection_range = Range {
                        start: offset_to_position(source_text, id_range.start().into()),
                        end: offset_to_position(source_text, id_range.end().into()),
                    };

                    let item = CallHierarchyItem {
                        name: method_name,
                        kind,
                        tags: None,
                        detail: Some(class_name),
                        uri: uri.clone(),
                        range,
                        selection_range,
                        data: Some(call_hierarchy_item_data(
                            method_fqn,
                            indexed.revision,
                            workspace_revision,
                        )),
                    };

                    return Ok(Some(vec![item]));
                }
                _ => {
                    node = if let Some(p) = node.parent() {
                        p
                    } else {
                        debug!("Reached root without finding method declaration");
                        return Ok(None);
                    };
                }
            }
        }
    }

    /// Get incoming calls (methods that call this method)
    pub fn incoming_calls(
        &self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>, String> {
        debug!("Getting incoming calls for {}", item.name);

        let target = validate_call_hierarchy_item(&self.workspace_index, item)?;
        let fqn = target.provenance.fqn.clone();

        debug!("Searching for callers of {}", fqn);

        let mut targets = vec![SymbolTarget::new(
            Arc::from(fqn.as_str()),
            DeclarationNamespace::for_kind(item.kind),
        )];
        // A call through a supertype (`Shape.area()`) may dispatch to this
        // override, so callers of the methods it overrides or implements are
        // callers of it too (IntelliJ shows them as well).
        if item.kind == tower_lsp::lsp_types::SymbolKind::METHOD {
            if let Ok(overridden) = self
                .workspace_index
                .find_current_overridden_declarations_cancellable(
                    &targets[0],
                    target.provenance.workspace_revision,
                    || false,
                )
            {
                targets.extend(overridden.into_iter().map(|definition| definition.target));
            }
        }
        let usages = self
            .workspace_index
            .find_current_usages_for_symbol_targets(
                &targets,
                Some(target.provenance.workspace_revision),
                || false,
            )
            .map_err(|error| format!("Exact incoming-call set unavailable: {error}"))?;
        debug!("Found {} usages", usages.locations().count());

        let mut incoming_calls: Vec<CallHierarchyIncomingCall> = Vec::new();
        // Map (name, uri, range-start-line, range-start-char, range-end-line, range-end-char) → index
        let mut incoming_index: HashMap<(String, Url, u32, u32, u32, u32), usize> = HashMap::new();

        for file in &usages.files {
            let indexed = self
                .workspace_index
                .get_indexed_revision_content(file.revision)
                .ok_or_else(|| "Incoming-call source changed during conversion".to_string())?;
            let source = Arc::clone(&indexed.content);

            for usage in &file.locations {
                // Convert usage range to LSP range
                let start_pos = offset_to_position(&source, usage.range.start as usize);
                let end_pos = offset_to_position(&source, usage.range.end as usize);
                let call_range = Range {
                    start: start_pos,
                    end: end_pos,
                };

                // Find the containing method for this call site. Dispatches on the
                // caller file's language: a Java method can be called from Kotlin,
                // and that caller's tree is not a Java tree.
                if let Some(containing_method) = self.containing_declaration_at_offset(
                    usage.file_id,
                    usage.range.start,
                    &source,
                )? {
                    let Some(caller_item) = self.item_for_containing_declaration(
                        containing_method,
                        &indexed,
                        usages.workspace_revision,
                    )?
                    else {
                        continue;
                    };

                    // Check if we already have this caller (O(1) lookup)
                    let key = (
                        caller_item.name.clone(),
                        caller_item.uri.clone(),
                        caller_item.range.start.line,
                        caller_item.range.start.character,
                        caller_item.range.end.line,
                        caller_item.range.end.character,
                    );
                    if let Some(&idx) = incoming_index.get(&key) {
                        incoming_calls[idx].from_ranges.push(call_range);
                    } else {
                        let idx = incoming_calls.len();
                        incoming_index.insert(key, idx);
                        incoming_calls.push(CallHierarchyIncomingCall {
                            from: caller_item,
                            from_ranges: vec![call_range],
                        });
                    }
                }
            }
        }

        debug!("Found {} incoming callers", incoming_calls.len());
        if self
            .workspace_index
            .current_usage_locations_are_current(&usages)
            && validate_call_hierarchy_item(&self.workspace_index, item).is_ok()
        {
            Ok(incoming_calls)
        } else {
            Err("Workspace changed while incoming calls were assembled".to_string())
        }
    }

    /// Get outgoing calls (methods called by this method)
    pub fn outgoing_calls(
        &self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, String> {
        debug!("Getting outgoing calls from {}", item.name);

        let source_item = validate_call_hierarchy_item(&self.workspace_index, item)?;
        let file_id = source_item.provenance.file_revision.file_id();
        if Language::from_path(&source_item.indexed.file_path) != Some(Language::Java) {
            return Err("Java outgoing call hierarchy received a non-Java item".to_string());
        }
        let source = source_item.indexed.content;
        let source_uri = Url::from_file_path(&source_item.indexed.file_path)
            .map_err(|()| "Call hierarchy source path is not a file URI".to_string())?;
        let (green, errors) = Parser::parse(Lexer::new(source.as_str()).collect());
        if !errors.is_empty() {
            return Err("Exact Java call hierarchy source has parse errors".to_string());
        }
        let root = SyntaxNode::new_root(green);

        // Find the method node at the item's range
        let offset = position_to_offset(&source, item.selection_range.start)?;
        let token = match root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()
        {
            Some(t) => t,
            None => return Ok(vec![]),
        };

        // Walk up to find the method declaration
        let mut node = match token.parent() {
            Some(n) => n,
            None => return Ok(vec![]),
        };

        let method_node = loop {
            match node.kind() {
                SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL => break node,
                _ => {
                    node = match node.parent() {
                        Some(p) => p,
                        None => return Ok(vec![]),
                    };
                }
            }
        };
        let method_name = self.extract_method_name_token(&method_node)?;
        let exact_fqn = self.exact_java_declaration_fqn(
            file_id,
            method_name.text(),
            method_name.text_range(),
            source_item.provenance.file_revision,
            source_item.provenance.workspace_revision,
            DeclarationNamespace::for_kind(if method_node.kind() == SyntaxKind::CONSTRUCTOR_DECL {
                SymbolKind::CONSTRUCTOR
            } else {
                SymbolKind::METHOD
            }),
        )?;
        if exact_fqn.as_deref() != Some(source_item.provenance.fqn.as_str()) {
            return Err("Call hierarchy item no longer identifies this Java overload".to_string());
        }

        // Find all method calls and constructor calls within this method
        let mut outgoing_calls: Vec<CallHierarchyOutgoingCall> = Vec::new();
        // Aggregate repeated call sites by declaration identity. Name + URI is
        // not an identity: one source file commonly owns same-named methods on
        // multiple nested/top-level types, as well as overload families.
        let mut outgoing_index: HashMap<(String, DeclarationNamespace), usize> = HashMap::new();

        for descendant in method_node.descendants() {
            match descendant.kind() {
                SyntaxKind::METHOD_CALL_EXPR => {
                    if let Some(call_info) = self.extract_method_call_info(&descendant) {
                        if let Some(called_method) = self.resolve_outgoing_target(
                            file_id,
                            &source,
                            &source_uri,
                            call_info.name_range,
                            Some(DeclarationNamespace::Callable),
                            source_item.provenance.file_revision,
                            source_item.provenance.workspace_revision,
                        )? {
                            let call_range = Range {
                                start: offset_to_position(&source, call_info.range.start as usize),
                                end: offset_to_position(&source, call_info.range.end as usize),
                            };

                            let key = outgoing_target_identity(&called_method)?;
                            if let Some(&idx) = outgoing_index.get(&key) {
                                outgoing_calls[idx].from_ranges.push(call_range);
                            } else {
                                let idx = outgoing_calls.len();
                                outgoing_index.insert(key, idx);
                                outgoing_calls.push(CallHierarchyOutgoingCall {
                                    to: called_method,
                                    from_ranges: vec![call_range],
                                });
                            }
                        }
                    }
                }
                SyntaxKind::NEW_EXPR => {
                    // Handle constructor calls: new ClassName(...)
                    if let Some(ctor_info) = self.extract_constructor_call_info(&descendant) {
                        if let Some(called_ctor) = self.resolve_outgoing_target(
                            file_id,
                            &source,
                            &source_uri,
                            ctor_info.name_range,
                            None,
                            source_item.provenance.file_revision,
                            source_item.provenance.workspace_revision,
                        )? {
                            let call_range = Range {
                                start: offset_to_position(&source, ctor_info.range.start as usize),
                                end: offset_to_position(&source, ctor_info.range.end as usize),
                            };

                            let key = outgoing_target_identity(&called_ctor)?;
                            if let Some(&idx) = outgoing_index.get(&key) {
                                outgoing_calls[idx].from_ranges.push(call_range);
                            } else {
                                let idx = outgoing_calls.len();
                                outgoing_index.insert(key, idx);
                                outgoing_calls.push(CallHierarchyOutgoingCall {
                                    to: called_ctor,
                                    from_ranges: vec![call_range],
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        debug!("Found {} outgoing calls", outgoing_calls.len());
        if validate_call_hierarchy_item(&self.workspace_index, item).is_ok()
            && outgoing_calls
                .iter()
                .all(|call| validate_call_hierarchy_item(&self.workspace_index, &call.to).is_ok())
        {
            Ok(outgoing_calls)
        } else {
            Err("Workspace changed while outgoing calls were assembled".to_string())
        }
    }

    /// Extract method name from a METHOD_DECL or CONSTRUCTOR_DECL node
    fn extract_method_name(
        &self,
        node: &hyperion_parser::syntax::SyntaxNode,
    ) -> Result<String, String> {
        self.extract_method_name_token(node)
            .map(|token| token.text().to_string())
    }

    fn extract_method_name_token(
        &self,
        node: &hyperion_parser::syntax::SyntaxNode,
    ) -> Result<hyperion_parser::SyntaxToken, String> {
        let mut name = None;
        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Node(child) if child.kind() == SyntaxKind::PARAM_LIST => break,
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::IDENT => {
                    name = Some(token);
                }
                _ => {}
            }
        }
        name.ok_or_else(|| "Could not find method name".to_string())
    }

    fn exact_java_declaration_fqn(
        &self,
        file_id: FileId,
        name: &str,
        name_range: rowan::TextRange,
        file_revision: FileRevisionToken,
        workspace_revision: WorkspaceRevision,
        namespace: DeclarationNamespace,
    ) -> Result<Option<String>, String> {
        let definition = self
            .workspace_index
            .find_symbol_definition_at_revision_range(
                file_revision,
                workspace_revision,
                TextRange::new(name_range.start().into(), name_range.end().into()),
                namespace,
            );
        Ok(definition
            .filter(|definition| {
                definition.location.file_id == file_id
                    && definition.symbol.simple_name.as_ref() == name
            })
            .map(|definition| definition.target.fqn.to_string()))
    }

    /// Find the containing class of a method
    fn find_containing_class(
        &self,
        method_node: &hyperion_parser::syntax::SyntaxNode,
    ) -> Result<(String, String), String> {
        let mut node = method_node.clone();

        while let Some(parent) = node.parent() {
            if parent.kind() == SyntaxKind::CLASS_DECL
                || parent.kind() == SyntaxKind::INTERFACE_DECL
                || parent.kind() == SyntaxKind::ENUM_DECL
            {
                // Extract class name
                for child in parent.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(token) = child {
                        if token.kind() == SyntaxKind::IDENT {
                            let class_name = token.text().to_string();
                            // Get package from the root compilation unit
                            let mut root = parent.clone();
                            while let Some(p) = root.parent() {
                                root = p;
                            }
                            let package = extract_package_from_root(&root);
                            let fqn = if let Some(pkg) = package {
                                format!("{pkg}.{class_name}")
                            } else {
                                class_name.clone()
                            };
                            return Ok((class_name, fqn));
                        }
                    }
                }
            }
            node = parent;
        }

        Err("Could not find containing class".to_string())
    }

    /// Find the declaration containing a given offset, in whatever language the
    /// file is written in.
    ///
    /// Incoming calls are looked up by FQN, and an FQN says nothing about the
    /// language of the files that reference it — a Kotlin function can be called
    /// from Java and vice versa. Everything except Kotlin keeps the Java walk it
    /// always used; Kotlin gets its own, because the two `SyntaxKind` enums are
    /// numbered independently and the Java walker would misread a Kotlin tree.
    pub(crate) fn containing_declaration_at_offset(
        &self,
        file_id: hyperion_workspace::FileId,
        offset: u32,
        source: &str,
    ) -> Result<Option<ContainingDeclaration>, String> {
        let is_kotlin = self
            .workspace_index
            .get_file_path(file_id)
            .and_then(|path| hyperion_workspace::Language::from_path(&path))
            .is_some_and(|language| language == hyperion_workspace::Language::Kotlin);

        if is_kotlin {
            return Ok(crate::call_hierarchy_kotlin::kotlin_containing_declaration(
                offset, source,
            ));
        }

        self.find_containing_method_at_offset(file_id, offset, source)
    }

    pub(crate) fn item_for_containing_declaration(
        &self,
        declaration: ContainingDeclaration,
        indexed: &IndexedContent,
        workspace_revision: WorkspaceRevision,
    ) -> Result<Option<CallHierarchyItem>, String> {
        let language = Language::from_path(&indexed.file_path).unwrap_or(Language::Java);
        let fqn = match language {
            Language::Kotlin => {
                if !crate::call_hierarchy_kotlin::kotlin_containing_declaration_is_unique(
                    indexed.content.as_str(),
                    &declaration,
                ) {
                    return Ok(None);
                }
                declaration.fqn.clone()
            }
            Language::Java => {
                let start = position_to_offset(
                    indexed.content.as_str(),
                    declaration.selection_range.start,
                )?;
                let end =
                    position_to_offset(indexed.content.as_str(), declaration.selection_range.end)?;
                let range = rowan::TextRange::new(start.into(), end.into());
                let Some(fqn) = self.exact_java_declaration_fqn(
                    indexed.revision.file_id(),
                    &declaration.name,
                    range,
                    indexed.revision,
                    workspace_revision,
                    DeclarationNamespace::for_kind(declaration.kind),
                )?
                else {
                    return Ok(None);
                };
                fqn
            }
            _ => return Ok(None),
        };
        let uri = Url::from_file_path(&indexed.file_path)
            .map_err(|()| "Caller source path is not a file URI".to_string())?;
        let data = if language == Language::Kotlin {
            let start = position_to_offset(indexed.content.as_str(), declaration.range.start)?;
            let end = position_to_offset(indexed.content.as_str(), declaration.range.end)?;
            crate::call_hierarchy_kotlin::kotlin_call_hierarchy_item_data(
                fqn,
                indexed.revision,
                workspace_revision,
                rowan::TextRange::new(start.into(), end.into()),
            )
        } else {
            call_hierarchy_item_data(fqn, indexed.revision, workspace_revision)
        };
        Ok(Some(CallHierarchyItem {
            name: declaration.name,
            kind: declaration.kind,
            tags: None,
            detail: declaration.container,
            uri,
            range: declaration.range,
            selection_range: declaration.selection_range,
            data: Some(data),
        }))
    }

    /// Find the method containing a given offset
    fn find_containing_method_at_offset(
        &self,
        _file_id: hyperion_workspace::FileId,
        offset: u32,
        source: &str,
    ) -> Result<Option<ContainingMethod>, String> {
        // Incoming ranges belong to `source`'s exact index revision. Parsing a
        // Salsa input for the same FileId could pair those ranges with another
        // publication, especially for closed caller files.
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        if !errors.is_empty() {
            return Ok(None);
        }
        let root = SyntaxNode::new_root(green);

        // Find token at offset
        let token = match root
            .token_at_offset(rowan::TextSize::from(offset))
            .right_biased()
        {
            Some(t) => t,
            None => return Ok(None),
        };

        // Walk up to find containing method
        let mut node = match token.parent() {
            Some(n) => n,
            None => return Ok(None),
        };

        loop {
            match node.kind() {
                SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL => {
                    let method_name = self.extract_method_name(&node)?;
                    let (class_name, class_fqn) = self.find_containing_class(&node)?;

                    let text_range = node.text_range();
                    let start_pos = offset_to_position(source, text_range.start().into());
                    let end_pos = offset_to_position(source, text_range.end().into());
                    let range = Range {
                        start: start_pos,
                        end: end_pos,
                    };

                    let name_token = self.extract_method_name_token(&node)?;
                    let id_range = name_token.text_range();
                    let selection_range = Range {
                        start: offset_to_position(source, id_range.start().into()),
                        end: offset_to_position(source, id_range.end().into()),
                    };

                    return Ok(Some(ContainingMethod {
                        name: method_name.clone(),
                        fqn: format!("{class_fqn}.{method_name}"),
                        kind: if node.kind() == SyntaxKind::CONSTRUCTOR_DECL {
                            SymbolKind::CONSTRUCTOR
                        } else {
                            SymbolKind::METHOD
                        },
                        container: Some(class_name),
                        range,
                        selection_range,
                    }));
                }
                SyntaxKind::CLASS_DECL
                | SyntaxKind::INTERFACE_DECL
                | SyntaxKind::ENUM_DECL
                | SyntaxKind::SOURCE_FILE => {
                    // Reached class/file level without finding method
                    // This might be a field initializer or static block
                    return Ok(None);
                }
                _ => {
                    node = match node.parent() {
                        Some(p) => p,
                        None => return Ok(None),
                    };
                }
            }
        }
    }

    /// Extract the exact source token which the shared navigation pipeline
    /// should resolve for this invocation.
    fn extract_method_call_info(&self, node: &SyntaxNode) -> Option<MethodCallInfo> {
        // Qualified calls store the method name as a direct IDENT and their
        // receiver as an expression child. Unqualified calls store the callee
        // in a NAME_EXPR (`init()` => NAME_EXPR("init") + ARG_LIST).
        let name_token = node
            .children_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| token.kind() == SyntaxKind::IDENT)
            .last()
            .or_else(|| {
                node.children()
                    .find(|child| child.kind() == SyntaxKind::NAME_EXPR)
                    .and_then(|callee| {
                        callee
                            .descendants_with_tokens()
                            .filter_map(rowan::NodeOrToken::into_token)
                            .find(|token| token.kind() == SyntaxKind::IDENT)
                    })
            })?;

        let text_range = node.text_range();
        let name_range = name_token.text_range();
        Some(MethodCallInfo {
            name_range: TextRange::new(name_range.start().into(), name_range.end().into()),
            range: TextRange {
                start: text_range.start().into(),
                end: text_range.end().into(),
            },
        })
    }

    /// Extract constructor call information from a NEW_EXPR node
    fn extract_constructor_call_info(&self, node: &SyntaxNode) -> Option<ConstructorCallInfo> {
        // The final identifier is the constructed type in both `new Widget()`
        // and `new right.Widget()`. Resolving the first identifier attributed
        // qualified constructor calls to a package or enclosing type.
        let name_token = node
            .children()
            .find(|child| child.kind() == SyntaxKind::CLASS_TYPE)?
            .descendants_with_tokens()
            .filter_map(rowan::NodeOrToken::into_token)
            .filter(|token| token.kind() == SyntaxKind::IDENT)
            .last()?;
        let text_range = node.text_range();
        let name_range = name_token.text_range();
        Some(ConstructorCallInfo {
            name_range: TextRange::new(name_range.start().into(), name_range.end().into()),
            range: TextRange {
                start: text_range.start().into(),
                end: text_range.end().into(),
            },
        })
    }

    /// Resolve an outgoing call through the same source-position-to-symbol
    /// identity boundary used by definition, references, rename, and hover.
    fn resolve_outgoing_target(
        &self,
        file_id: FileId,
        source: &str,
        uri: &Url,
        name_range: TextRange,
        expected_namespace: Option<DeclarationNamespace>,
        request_revision: FileRevisionToken,
        workspace_revision: WorkspaceRevision,
    ) -> Result<Option<CallHierarchyItem>, String> {
        let position = offset_to_position(source, name_range.start as usize);
        let Some(resolved) = self
            .navigation_provider
            .resolve_workspace_target_at_position(
                file_id,
                position,
                source,
                uri,
                expected_namespace,
            )?
        else {
            return Ok(None);
        };
        if resolved.request_revision != request_revision
            || resolved.definition.workspace_revision != workspace_revision
        {
            return Err("Workspace changed while resolving an outgoing target".to_string());
        }
        if expected_namespace.is_none()
            && !matches!(
                resolved.definition.target.namespace,
                DeclarationNamespace::Constructor | DeclarationNamespace::Type
            )
        {
            return Ok(None);
        }
        self.create_call_hierarchy_item_from_location(
            &resolved.definition.location,
            &resolved.definition.target.fqn,
            workspace_revision,
        )
    }

    /// Create a CallHierarchyItem from a SymbolLocation
    fn create_call_hierarchy_item_from_location(
        &self,
        location: &hyperion_workspace::SymbolLocation,
        fqn: &str,
        workspace_revision: WorkspaceRevision,
    ) -> Result<Option<CallHierarchyItem>, String> {
        let Some(revisioned) = self.workspace_index.revisioned_location(location) else {
            return Ok(None);
        };
        let Some(indexed) = self
            .workspace_index
            .get_indexed_revision_content(revisioned.revision)
        else {
            return Ok(None);
        };
        if !self
            .workspace_index
            .workspace_revisions_are_current(workspace_revision, &[revisioned.revision])
        {
            return Err("Workspace changed while resolving an outgoing target".to_string());
        }
        let uri = Url::from_file_path(&indexed.file_path).map_err(|()| {
            format!(
                "Failed to create URI from path: {}",
                indexed.file_path.display()
            )
        })?;
        let source = indexed.content;

        let start_pos = offset_to_position(&source, location.range.start as usize);
        let end_pos = offset_to_position(&source, location.range.end as usize);
        let range = Range {
            start: start_pos,
            end: end_pos,
        };

        Ok(Some(CallHierarchyItem {
            name: location.name.to_string(),
            kind: location.kind,
            tags: None,
            detail: location.container_name.as_ref().map(|s| s.to_string()),
            uri,
            range,
            selection_range: range,
            data: Some(call_hierarchy_item_data(
                fqn,
                revisioned.revision,
                workspace_revision,
            )),
        }))
    }
}

/// Information about a containing method.
///
/// Shared with [`crate::call_hierarchy_kotlin`], which produces one of these for
/// a Kotlin caller so both languages feed the same incoming-calls loop.
pub(crate) struct ContainingDeclaration {
    pub name: String,
    pub fqn: String,
    pub kind: SymbolKind,
    pub container: Option<String>,
    pub range: Range,
    pub selection_range: Range,
}

/// Java-only alias kept for the existing Java walk's return type.
type ContainingMethod = ContainingDeclaration;

/// Information about a method call
struct MethodCallInfo {
    name_range: TextRange,
    range: TextRange,
}

/// Information about a constructor call (new ClassName())
struct ConstructorCallInfo {
    name_range: TextRange,
    range: TextRange,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_host::{ReadTxn, SemanticHost};
    use hyperion_semantics::db::SourceDatabase;
    use tempfile::TempDir;

    struct JavaFixture {
        _temp: TempDir,
        path: std::path::PathBuf,
        source: String,
        file_id: FileId,
        index: Arc<WorkspaceIndex>,
        provider: CallHierarchyProvider<'static>,
        uri: Url,
    }

    fn java_fixture(source: &str) -> JavaFixture {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Subject.java");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&path, source)
            .expect("index Java fixture");
        let file_id = index.get_file_id(&path).expect("fixture FileId");
        let host = SemanticHost::new(Arc::clone(&index));
        host.with_db(|db| {
            db.set_file_text_tracked(file_id, Arc::new(source.to_owned()));
            db.set_file_language(file_id, Language::Java);
        });
        // The fixture outlives every borrow of its own read transaction, so the
        // transaction is leaked for the (test-only) duration of the process.
        let txn: &'static ReadTxn = Box::leak(Box::new(host.read_now()));
        let provider = CallHierarchyProvider::new(txn, Arc::clone(&index));
        let uri = Url::from_file_path(&path).expect("fixture URI");
        JavaFixture {
            _temp: temp,
            path,
            source: source.to_owned(),
            file_id,
            index,
            provider,
            uri,
        }
    }

    fn position_of(source: &str, needle: &str) -> Position {
        offset_to_position(source, source.find(needle).expect("fixture marker"))
    }

    fn prepared_item(fixture: &JavaFixture, marker: &str) -> CallHierarchyItem {
        fixture
            .provider
            .prepare(
                fixture.file_id,
                position_of(&fixture.source, marker),
                &fixture.source,
                &fixture.uri,
            )
            .expect("prepare succeeds")
            .expect("item exists")
            .into_iter()
            .next()
            .expect("one item")
    }

    #[test]
    fn java_prepare_carries_exact_overload_and_string_revisions() {
        let fixture = java_fixture(
            "package p; class Subject { void pick(int value) {} void pick(String value) {} }",
        );
        let integer = prepared_item(&fixture, "pick(int");
        let string = prepared_item(&fixture, "pick(String");

        assert_eq!(item_fqn(&integer).unwrap(), "p.Subject.pick(int)");
        assert_eq!(
            item_fqn(&string).unwrap(),
            "p.Subject.pick(java.lang.String)"
        );
        assert_ne!(item_fqn(&integer).unwrap(), item_fqn(&string).unwrap());
        for item in [&integer, &string] {
            let data = item.data.as_ref().expect("item data");
            assert!(data["file_revision"].is_string());
            assert!(data["workspace_revision"].is_string());
        }
    }

    #[test]
    fn java_prepare_distinguishes_constructor_from_same_fqn_method() {
        let fixture = java_fixture(concat!(
            "package p; class Subject { ",
            "Subject() {} void Subject() {} }",
        ));
        let offsets = fixture
            .source
            .match_indices("Subject()")
            .map(|(offset, _)| offset)
            .collect::<Vec<_>>();
        assert_eq!(offsets.len(), 2);
        let prepare = |offset| {
            fixture
                .provider
                .prepare(
                    fixture.file_id,
                    offset_to_position(&fixture.source, offset),
                    &fixture.source,
                    &fixture.uri,
                )
                .unwrap()
                .unwrap()
                .remove(0)
        };

        let constructor = prepare(offsets[0]);
        let method = prepare(offsets[1]);
        assert_eq!(constructor.kind, SymbolKind::CONSTRUCTOR);
        assert_eq!(method.kind, SymbolKind::METHOD);
        assert_eq!(item_fqn(&constructor).unwrap(), "p.Subject.Subject()");
        assert_eq!(item_fqn(&method).unwrap(), "p.Subject.Subject()");
    }

    #[test]
    fn java_outgoing_dedup_keeps_same_named_targets_in_one_file_distinct() {
        let fixture = java_fixture(concat!(
            "package p; ",
            "class Subject { void source(p.Left left, p.Right right) { ",
            "left.run(); right.run(); left.run(); } } ",
            "class Left { void run() {} } ",
            "class Right { void run() {} }",
        ));
        let source = prepared_item(&fixture, "source(");

        let outgoing = fixture
            .provider
            .outgoing_calls(&source)
            .expect("outgoing calls should resolve");
        assert_eq!(
            outgoing.len(),
            2,
            "declarations must not collapse by name + URI"
        );

        let mut identities = outgoing
            .iter()
            .map(|call| (item_fqn(&call.to).unwrap(), call.from_ranges.len()))
            .collect::<Vec<_>>();
        identities.sort_unstable();
        assert_eq!(
            identities,
            vec![
                ("p.Left.run()".to_string(), 2),
                ("p.Right.run()".to_string(), 1)
            ]
        );
    }

    #[test]
    fn same_text_republication_retires_a_prepared_item() {
        let fixture = java_fixture("package p; class Subject { void run() {} }");
        let item = prepared_item(&fixture, "run");
        let old_revision = encoded_item_revision(&item, "file_revision").unwrap();

        fixture
            .index
            .index_file_with_source(&fixture.path, &fixture.source)
            .expect("same-text R2 publication");
        assert_ne!(
            fixture
                .index
                .current_file_revision(fixture.file_id)
                .unwrap()
                .revision_id()
                .get(),
            old_revision
        );
        assert!(fixture.provider.incoming_calls(&item).is_err());
        assert!(fixture.provider.outgoing_calls(&item).is_err());
    }

    #[test]
    fn unrelated_workspace_publication_retires_a_prepared_item() {
        let fixture = java_fixture("package p; class Subject { void run() {} }");
        let item = prepared_item(&fixture, "run");
        fixture
            .index
            .index_file_with_source(
                &fixture.path.with_file_name("Other.java"),
                "package p; class Other {}",
            )
            .expect("unrelated workspace publication");

        assert!(fixture.provider.incoming_calls(&item).is_err());
        assert!(fixture.provider.outgoing_calls(&item).is_err());
    }

    #[test]
    fn incoming_caller_is_parsed_from_its_exact_indexed_bytes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target_path = temp.path().join("Target.java");
        let caller_path = temp.path().join("Caller.java");
        let target = "package p; class Target { void target() {} }";
        let caller = concat!(
            "package p; class Caller { void actualCaller() { ",
            "new Target().target(); } }",
        );
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        for _ in 0..2 {
            index
                .index_file_with_source(&target_path, target)
                .expect("index target");
            index
                .index_file_with_source(&caller_path, caller)
                .expect("index caller");
        }
        let target_id = index.get_file_id(&target_path).unwrap();
        let caller_id = index.get_file_id(&caller_path).unwrap();
        let host = SemanticHost::new(Arc::clone(&index));
        host.with_db(|db| {
            db.set_file_text_tracked(
                caller_id,
                Arc::new("package p; class Caller { void staleSalsaCaller() {} }".to_string()),
            );
            db.set_file_language(caller_id, Language::Java);
        });
        let txn = host.read_now();
        let provider = CallHierarchyProvider::new(&txn, Arc::clone(&index));
        let target_uri = Url::from_file_path(&target_path).unwrap();
        let item = provider
            .prepare(
                target_id,
                position_of(target, "target()"),
                target,
                &target_uri,
            )
            .unwrap()
            .unwrap()
            .remove(0);

        let incoming = provider.incoming_calls(&item).expect("incoming calls");
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].from.name, "actualCaller");
        assert_eq!(
            item_fqn(&incoming[0].from).unwrap(),
            "p.Caller.actualCaller()"
        );
    }
}
