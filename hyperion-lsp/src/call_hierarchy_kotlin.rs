// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin call hierarchy (`callHierarchy/*`).
//!
//! The Java provider in [`crate::call_hierarchy`] decodes rowan kinds through
//! `hyperion_parser::SyntaxKind`; a Kotlin green tree carries
//! `hyperion_kotlin::SyntaxKind` values with different numbering, so it has to be
//! walked separately. What *is* shared with Java is the data source: both
//! directions are built from the same workspace index (`find_usages` for
//! incoming, symbol lookup for outgoing), and that is what makes cross-language
//! edges fall out for free.
//!
//! # What can start a hierarchy
//!
//! `prepare` answers for a cursor sitting on the *name* of
//!
//! * a `fun` declaration — top level, member, or local;
//! * a secondary `constructor(…)`;
//! * a property accessor (`get()` / `set(value)`), which reports the property;
//! * a `class` name, which in Kotlin *is* the primary constructor, so its
//!   incoming calls are the construction sites. An `object` has no callable
//!   constructor and does not start a hierarchy.
//!
//! A cursor on a hard keyword (`fun`, `val`, `class`, …) yields nothing: the
//! request only means something against a named declaration.
//!
//! # Cross-language edges
//!
//! Kotlin FQNs are `container.name` with no parameter descriptor (see
//! `hyperion_workspace::kotlin_usage_extractor`), and the Java usage extractor
//! resolves receiver types through the same index, so a Java call to a Kotlin
//! function is recorded under the Kotlin FQN. Incoming calls therefore only have
//! to decide, *per caller file*, which CST to walk — a decision made once in
//! [`crate::call_hierarchy::CallHierarchyProvider::containing_declaration_at_offset`],
//! which routes `.kt` callers to [`kotlin_containing_declaration`] here and
//! leaves every other file on the Java walker it always used.

use hyperion_kotlin::{SyntaxKind as K, SyntaxNode, SyntaxToken};
use hyperion_parser::{SyntaxKind as JavaKind, SyntaxNode as JavaSyntaxNode};
use hyperion_semantics::db::FileId;
use hyperion_workspace::{
    DeclarationNamespace, FileRevisionToken, Language, QueryEngine, ResolvedDefinition,
    SymbolTarget, WorkspaceIndex, WorkspaceRevision,
};
use rowan::{TextRange, TextSize};
use std::collections::HashMap;
use std::sync::Arc;
use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Position, Range,
    SymbolKind, Url,
};
use tracing::debug;

use crate::call_hierarchy::{
    call_hierarchy_item_data, exact_call_hierarchy_source, item_fqn, validate_call_hierarchy_item,
    CallHierarchyProvider, ContainingDeclaration,
};
use crate::kotlin_calls::{analyze_call, is_name_token, KotlinCall};
use crate::navigation::NavigationProvider;
use crate::navigation_util::{offset_to_position, position_to_offset};
use crate::semantic_host::ReadTxn;

/// Call hierarchy for Kotlin buffers.
pub struct KotlinCallHierarchyProvider<'txn> {
    txn: &'txn ReadTxn,
    workspace_index: Arc<WorkspaceIndex>,
    navigation_provider: NavigationProvider<'txn>,
}

const DATA_KOTLIN_DECLARATION_START: &str = "kotlin_declaration_start";
const DATA_KOTLIN_DECLARATION_END: &str = "kotlin_declaration_end";

/// Add the exact Kotlin declaration span to the ordinary revision proof.
/// Kotlin callable FQNs are descriptorless, and every constructor shares its
/// owning class FQN, so the span is part of the semantic identity rather than
/// presentation-only metadata.
pub(crate) fn kotlin_call_hierarchy_item_data(
    fqn: impl Into<String>,
    file_revision: FileRevisionToken,
    workspace_revision: WorkspaceRevision,
    declaration_range: TextRange,
) -> serde_json::Value {
    let mut data = call_hierarchy_item_data(fqn, file_revision, workspace_revision);
    let object = data
        .as_object_mut()
        .expect("call hierarchy data is always an object");
    object.insert(
        DATA_KOTLIN_DECLARATION_START.to_string(),
        serde_json::json!(u32::from(declaration_range.start())),
    );
    object.insert(
        DATA_KOTLIN_DECLARATION_END.to_string(),
        serde_json::json!(u32::from(declaration_range.end())),
    );
    data
}

fn encoded_kotlin_declaration_range(item: &CallHierarchyItem) -> Result<TextRange, String> {
    let offset = |key: &str| {
        let raw = item
            .data
            .as_ref()
            .and_then(|data| data.get(key))
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("Missing {key} in Kotlin call hierarchy item data"))?;
        u32::try_from(raw)
            .map(TextSize::from)
            .map_err(|_| format!("Invalid {key} in Kotlin call hierarchy item data"))
    };
    let start = offset(DATA_KOTLIN_DECLARATION_START)?;
    let end = offset(DATA_KOTLIN_DECLARATION_END)?;
    (start < end)
        .then(|| TextRange::new(start, end))
        .ok_or_else(|| "Invalid Kotlin declaration range in call hierarchy item data".to_string())
}

impl<'txn> KotlinCallHierarchyProvider<'txn> {
    pub fn new(txn: &'txn ReadTxn, workspace_index: Arc<WorkspaceIndex>) -> Self {
        let navigation_provider = NavigationProvider::new(
            txn,
            Arc::clone(&workspace_index),
            Arc::new(QueryEngine::new(workspace_index.as_ref().clone())),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        Self {
            txn,
            workspace_index,
            navigation_provider,
        }
    }

    // -----------------------------------------------------------------------
    // prepare
    // -----------------------------------------------------------------------

    /// The callable under the cursor, or `None` when the cursor is not on the
    /// name of one.
    pub fn prepare(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Vec<CallHierarchyItem>>, String> {
        let (indexed, workspace_revision) =
            exact_call_hierarchy_source(&self.workspace_index, file_id, source_text, uri)?;
        if Language::from_path(&indexed.file_path) != Some(Language::Kotlin) {
            return Err("Kotlin call hierarchy received a non-Kotlin source".to_string());
        }
        let offset = position_to_offset(source_text, position)
            .map_err(|e| format!("Failed to convert position: {e}"))?;

        let (green, errors) =
            hyperion_kotlin::Parser::parse(hyperion_kotlin::Lexer::new(source_text).collect());
        if !errors.is_empty() {
            return Ok(None);
        }
        let root = SyntaxNode::new_root(green);

        let Some(declaration) = declaration_at_offset(&root, offset) else {
            debug!("Kotlin call hierarchy: no callable at offset {offset}");
            return Ok(None);
        };
        if !declaration_identity_is_unique(&root, &declaration) {
            debug!("Kotlin call hierarchy: overload identity is ambiguous");
            return Ok(None);
        }

        Ok(Some(vec![declaration.into_item(
            source_text,
            uri.clone(),
            indexed.revision,
            workspace_revision,
        )]))
    }

    // -----------------------------------------------------------------------
    // incoming
    // -----------------------------------------------------------------------

    /// Callers of `item`, from the workspace usage records.
    ///
    /// `from_ranges` are the call-site ranges inside each caller — the ranges
    /// the index recorded for the reference itself, not the caller's own span.
    pub fn incoming_calls(
        &self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>, String> {
        let target = validate_call_hierarchy_item(&self.workspace_index, item)?;
        let fqn = target.provenance.fqn.clone();
        debug!("Kotlin call hierarchy: searching for callers of {fqn}");
        if Language::from_path(&target.indexed.file_path) != Some(Language::Kotlin) {
            return Err("Kotlin incoming call hierarchy received a non-Kotlin item".to_string());
        }
        let (green, errors) = hyperion_kotlin::Parser::parse(
            hyperion_kotlin::Lexer::new(target.indexed.content.as_str()).collect(),
        );
        if !errors.is_empty() {
            return Err("Exact Kotlin call hierarchy source has parse errors".to_string());
        }
        let target_root = SyntaxNode::new_root(green);
        let (target_node, _) =
            validate_kotlin_declaration(&target_root, target.indexed.content.as_str(), item, &fqn)?;
        let constructor_selection = (item.kind == SymbolKind::CONSTRUCTOR).then(|| {
            let shapes = constructor_shapes_for_declaration(&target_node);
            let selected = target_node.text_range();
            (selected, shapes)
        });

        // The caller-side walk is language-dispatched and lives on the Java
        // provider; reuse it rather than keeping a second copy in step.
        let callers = CallHierarchyProvider::new(self.txn, Arc::clone(&self.workspace_index));

        let mut incoming: Vec<CallHierarchyIncomingCall> = Vec::new();
        let mut seen: HashMap<CallerKey, usize> = HashMap::new();

        let usages = self
            .workspace_index
            .find_current_usages_for_symbol_targets(
                &[SymbolTarget::new(
                    Arc::from(fqn.as_str()),
                    if item.kind == SymbolKind::CONSTRUCTOR {
                        // Kotlin construction sites resolve to the class
                        // declaration itself: unlike Java constructors, the
                        // indexed semantic identity is in the type namespace.
                        DeclarationNamespace::Type
                    } else {
                        DeclarationNamespace::for_kind(item.kind)
                    },
                )],
                Some(target.provenance.workspace_revision),
                || false,
            )
            .map_err(|error| format!("Exact incoming-call set unavailable: {error}"))?;
        for file in &usages.files {
            let indexed = self
                .workspace_index
                .get_indexed_revision_content(file.revision)
                .ok_or_else(|| "Incoming-call source changed during conversion".to_string())?;
            let source = Arc::clone(&indexed.content);
            let caller_language = Language::from_path(&indexed.file_path);
            let (kotlin_constructor_root, java_constructor_root) =
                if constructor_selection.is_some() {
                    match caller_language {
                        Some(Language::Kotlin) => {
                            let (green, errors) = hyperion_kotlin::Parser::parse(
                                hyperion_kotlin::Lexer::new(source.as_str()).collect(),
                            );
                            if !errors.is_empty() {
                                return Err("Exact Kotlin incoming-call source has parse errors"
                                    .to_string());
                            }
                            (Some(SyntaxNode::new_root(green)), None)
                        }
                        Some(Language::Java) => {
                            let (green, errors) = hyperion_parser::Parser::parse(
                                hyperion_parser::Lexer::new(source.as_str()).collect(),
                            );
                            if !errors.is_empty() {
                                return Err(
                                    "Exact Java incoming-call source has parse errors".to_string()
                                );
                            }
                            (None, Some(JavaSyntaxNode::new_root(green)))
                        }
                        _ => (None, None),
                    }
                } else {
                    (None, None)
                };

            for usage in &file.locations {
                if let Some((selected, shapes)) = &constructor_selection {
                    let usage_range =
                        TextRange::new(usage.range.start.into(), usage.range.end.into());
                    let resolved = match caller_language {
                        Some(Language::Kotlin) => kotlin_constructor_root
                            .as_ref()
                            .and_then(|root| constructor_call_at_usage(root, usage_range))
                            .and_then(|call| {
                                unique_constructor_shape(shapes, |shape| {
                                    constructor_shape_accepts_call(shape, &call)
                                })
                            }),
                        Some(Language::Java) => java_constructor_root
                            .as_ref()
                            .and_then(|root| {
                                java_constructor_argument_count_at_usage(root, usage_range)
                            })
                            .and_then(|argument_count| {
                                unique_constructor_shape(shapes, |shape| {
                                    constructor_shape_accepts_java_arity(shape, argument_count)
                                })
                            }),
                        _ => None,
                    };
                    let Some(resolved) = resolved else {
                        continue;
                    };
                    if resolved.declaration_range != *selected {
                        continue;
                    }
                }
                let call_range = Range {
                    start: offset_to_position(&source, usage.range.start as usize),
                    end: offset_to_position(&source, usage.range.end as usize),
                };

                let Some(caller) = callers.containing_declaration_at_offset(
                    usage.file_id,
                    usage.range.start,
                    &source,
                )?
                else {
                    continue;
                };

                let Some(from) = callers.item_for_containing_declaration(
                    caller,
                    &indexed,
                    usages.workspace_revision,
                )?
                else {
                    continue;
                };

                let key = caller_key(&from);
                if let Some(&existing) = seen.get(&key) {
                    incoming[existing].from_ranges.push(call_range);
                } else {
                    seen.insert(key, incoming.len());
                    incoming.push(CallHierarchyIncomingCall {
                        from,
                        from_ranges: vec![call_range],
                    });
                }
            }
        }

        debug!(
            "Kotlin call hierarchy: {} incoming caller(s)",
            incoming.len()
        );
        if self
            .workspace_index
            .current_usage_locations_are_current(&usages)
            && validate_call_hierarchy_item(&self.workspace_index, item).is_ok()
        {
            Ok(incoming)
        } else {
            Err("Workspace changed while incoming calls were assembled".to_string())
        }
    }

    // -----------------------------------------------------------------------
    // outgoing
    // -----------------------------------------------------------------------

    /// Calls made from the body of `item`.
    pub fn outgoing_calls(
        &self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, String> {
        let source_item = validate_call_hierarchy_item(&self.workspace_index, item)?;
        let file_id = source_item.provenance.file_revision.file_id();
        if Language::from_path(&source_item.indexed.file_path) != Some(Language::Kotlin) {
            return Err("Kotlin outgoing call hierarchy received a non-Kotlin item".to_string());
        }
        let source = source_item.indexed.content;
        let source_uri = Url::from_file_path(&source_item.indexed.file_path)
            .map_err(|()| "Kotlin call hierarchy source path is not a file URI".to_string())?;
        let (green, errors) =
            hyperion_kotlin::Parser::parse(hyperion_kotlin::Lexer::new(source.as_str()).collect());
        if !errors.is_empty() {
            return Err("Exact Kotlin call hierarchy source has parse errors".to_string());
        }
        let root = SyntaxNode::new_root(green);

        let (body, _) =
            validate_kotlin_declaration(&root, &source, item, &source_item.provenance.fqn)?;

        let mut outgoing: Vec<CallHierarchyOutgoingCall> = Vec::new();
        let mut seen: HashMap<(String, DeclarationNamespace), usize> = HashMap::new();

        let mut call_exprs = Vec::new();
        collect_call_exprs(&body, &mut call_exprs);
        for descendant in call_exprs {
            let Some(call) = analyze_call(&descendant) else {
                continue;
            };
            let Some(callee_range) = callee_name_range(&call) else {
                continue;
            };
            let position = offset_to_position(&source, usize::from(callee_range.start()));
            let Some(resolved) = self
                .navigation_provider
                .resolve_workspace_target_at_position(
                    file_id,
                    position,
                    &source,
                    &source_uri,
                    None,
                )?
            else {
                continue;
            };
            if resolved.request_revision != source_item.provenance.file_revision
                || resolved.definition.workspace_revision
                    != source_item.provenance.workspace_revision
            {
                return Err(
                    "Workspace changed while resolving a Kotlin outgoing target".to_string()
                );
            }
            if !matches!(
                resolved.definition.target.namespace,
                DeclarationNamespace::Callable
                    | DeclarationNamespace::Constructor
                    | DeclarationNamespace::Type
            ) {
                continue;
            }
            let Some(target) = self.item_for_definition(
                resolved.definition,
                source_item.provenance.workspace_revision,
            ) else {
                continue;
            };

            let from_range = Range {
                start: offset_to_position(&source, usize::from(callee_range.start())),
                end: offset_to_position(&source, usize::from(callee_range.end())),
            };

            let key = (
                item_fqn(&target)?,
                DeclarationNamespace::for_kind(target.kind),
            );
            if let Some(&existing) = seen.get(&key) {
                outgoing[existing].from_ranges.push(from_range);
            } else {
                seen.insert(key, outgoing.len());
                outgoing.push(CallHierarchyOutgoingCall {
                    to: target,
                    from_ranges: vec![from_range],
                });
            }
        }

        debug!("Kotlin call hierarchy: {} outgoing call(s)", outgoing.len());
        if validate_call_hierarchy_item(&self.workspace_index, item).is_ok()
            && outgoing
                .iter()
                .all(|call| validate_call_hierarchy_item(&self.workspace_index, &call.to).is_ok())
        {
            Ok(outgoing)
        } else {
            Err("Workspace changed while Kotlin outgoing calls were assembled".to_string())
        }
    }

    fn item_for_definition(
        &self,
        definition: ResolvedDefinition,
        workspace_revision: WorkspaceRevision,
    ) -> Option<CallHierarchyItem> {
        if definition.workspace_revision != workspace_revision {
            return None;
        }
        let revisioned = definition.revisioned_location()?;
        let indexed = self
            .workspace_index
            .get_indexed_revision_content(revisioned.revision)?;
        let location = definition.location;
        if Language::from_path(&indexed.file_path) == Some(Language::Kotlin) {
            let (green, errors) = hyperion_kotlin::Parser::parse(
                hyperion_kotlin::Lexer::new(indexed.content.as_str()).collect(),
            );
            if !errors.is_empty() {
                return None;
            }
            let root = SyntaxNode::new_root(green);
            let hierarchy_kind = if is_type(location.kind) {
                SymbolKind::CONSTRUCTOR
            } else {
                location.kind
            };
            let mut declarations =
                kotlin_declarations_with_identity(&root, &definition.target.fqn, hierarchy_kind);
            if declarations.len() != 1 {
                return None;
            }
            if !self
                .workspace_index
                .workspace_revisions_are_current(workspace_revision, &[revisioned.revision])
            {
                return None;
            }
            let uri = Url::from_file_path(&indexed.file_path).ok()?;
            return Some(declarations.pop()?.into_item(
                indexed.content.as_str(),
                uri,
                revisioned.revision,
                workspace_revision,
            ));
        }
        if !self
            .workspace_index
            .workspace_revisions_are_current(workspace_revision, &[revisioned.revision])
        {
            return None;
        }
        let uri = Url::from_file_path(&indexed.file_path).ok()?;
        let range = Range {
            start: offset_to_position(&indexed.content, location.range.start as usize),
            end: offset_to_position(&indexed.content, location.range.end as usize),
        };

        Some(CallHierarchyItem {
            name: location.name.to_string(),
            kind: if is_type(location.kind) {
                SymbolKind::CONSTRUCTOR
            } else {
                location.kind
            },
            tags: None,
            detail: location.container_name.as_ref().map(ToString::to_string),
            uri,
            range,
            selection_range: range,
            data: Some(call_hierarchy_item_data(
                definition.target.fqn.to_string(),
                revisioned.revision,
                workspace_revision,
            )),
        })
    }
}

// ---------------------------------------------------------------------------
// Caller-side walk, shared with the Java provider
// ---------------------------------------------------------------------------

/// The Kotlin declaration containing `offset`, for the incoming-calls walk.
///
/// Parses `source` directly instead of going through Salsa: caller files are
/// arbitrary workspace files, and most of them are not open — so most of them
/// are not registered as Salsa inputs.
pub(crate) fn kotlin_containing_declaration(
    offset: u32,
    source: &str,
) -> Option<ContainingDeclaration> {
    let tokens = hyperion_kotlin::Lexer::new(source).collect();
    let (green, errors) = hyperion_kotlin::Parser::parse(tokens);
    if !errors.is_empty() {
        return None;
    }
    let root = SyntaxNode::new_root(green);
    let package = package_of(&root);

    let token = token_at(&root, offset)?;
    let mut node = token.parent()?;
    loop {
        match node.kind() {
            K::FUNCTION_DECL | K::CONSTRUCTOR_DECL | K::PROPERTY_ACCESSOR | K::PROPERTY_DECL => {
                return KotlinDeclaration::from_node(&node, package.as_deref())
                    .map(|declaration| declaration.into_containing(source));
            }
            // A call directly inside a class body — a property initialiser or an
            // `init` block — belongs to the type's constructor.
            K::CLASS_DECL => {
                return KotlinDeclaration::from_node(&node, package.as_deref())
                    .map(KotlinDeclaration::into_constructor)
                    .map(|declaration| declaration.into_containing(source));
            }
            K::OBJECT_DECL => return None,
            K::SOURCE_FILE => return None,
            _ => node = node.parent()?,
        }
    }
}

pub(crate) fn kotlin_containing_declaration_is_unique(
    source: &str,
    declaration: &ContainingDeclaration,
) -> bool {
    let (green, errors) =
        hyperion_kotlin::Parser::parse(hyperion_kotlin::Lexer::new(source).collect());
    if !errors.is_empty() {
        return false;
    }
    let root = SyntaxNode::new_root(green);
    let Ok(start) = position_to_offset(source, declaration.range.start) else {
        return false;
    };
    let Ok(end) = position_to_offset(source, declaration.range.end) else {
        return false;
    };
    let range = TextRange::new(start.into(), end.into());
    let package = package_of(&root);
    root.descendants()
        .find(|node| node.text_range() == range)
        .and_then(|node| kotlin_declaration_for_node(&node, package.as_deref(), declaration.kind))
        .is_some_and(|candidate| {
            candidate.fqn == declaration.fqn
                && candidate.name == declaration.name
                && declaration_identity_is_unique(&root, &candidate)
        })
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

/// A Kotlin declaration that can anchor a call hierarchy.
struct KotlinDeclaration {
    name: String,
    fqn: String,
    kind: SymbolKind,
    /// Simple name of the enclosing type, shown as the item's detail.
    container: Option<String>,
    /// Span of the whole declaration.
    range: rowan::TextRange,
    /// Span of just the name.
    selection_range: rowan::TextRange,
}

impl KotlinDeclaration {
    /// Read a declaration off its CST node, or `None` if it has no usable name.
    fn from_node(node: &SyntaxNode, package: Option<&str>) -> Option<Self> {
        let owners = enclosing_type_chain(node);

        let (name, selection_range, kind) = match node.kind() {
            K::FUNCTION_DECL => {
                let token = function_name_token(node)?;
                let kind = if owners.is_empty() {
                    SymbolKind::FUNCTION
                } else {
                    SymbolKind::METHOD
                };
                (token.text().to_string(), token.text_range(), kind)
            }
            K::CONSTRUCTOR_DECL => {
                let name = owners.last().cloned()?;
                let token = node
                    .children_with_tokens()
                    .filter_map(|child| child.into_token())
                    .find(|token| token.kind() == K::CONSTRUCTOR_KW)?;
                (name, token.text_range(), SymbolKind::CONSTRUCTOR)
            }
            // An accessor is not separately indexed: it reports the property it
            // belongs to, whose FQN is what property reads are recorded under.
            K::PROPERTY_ACCESSOR => {
                let property = accessor_property(node)?;
                let token = node
                    .children_with_tokens()
                    .filter_map(|child| child.into_token())
                    .find(|token| is_name_token(token.kind()))?;
                (property, token.text_range(), SymbolKind::PROPERTY)
            }
            K::PROPERTY_DECL => {
                let token = property_name_token(node)?;
                (
                    token.text().to_string(),
                    token.text_range(),
                    SymbolKind::PROPERTY,
                )
            }
            K::CLASS_DECL | K::OBJECT_DECL | K::INTERFACE_DECL => {
                let token = type_name_token(node)?;
                (
                    token.text().to_string(),
                    token.text_range(),
                    SymbolKind::CLASS,
                )
            }
            _ => return None,
        };

        let fqn = if node.kind() == K::CONSTRUCTOR_DECL {
            join_fqn(package, &owners[..owners.len().saturating_sub(1)], &name)
        } else {
            join_fqn(package, &owners, &name)
        };

        Some(Self {
            fqn,
            name,
            kind,
            container: owners.last().cloned(),
            range: node.text_range(),
            selection_range,
        })
    }

    /// Re-label a type declaration as its primary constructor.
    ///
    /// In Kotlin the class name *is* the constructor, and the class FQN is also
    /// what the usage index records construction sites under.
    const fn into_constructor(mut self) -> Self {
        self.kind = SymbolKind::CONSTRUCTOR;
        self
    }

    fn into_item(
        self,
        source: &str,
        uri: Url,
        file_revision: FileRevisionToken,
        workspace_revision: WorkspaceRevision,
    ) -> CallHierarchyItem {
        let data = kotlin_call_hierarchy_item_data(
            self.fqn.clone(),
            file_revision,
            workspace_revision,
            self.range,
        );
        CallHierarchyItem {
            name: self.name,
            kind: self.kind,
            tags: None,
            detail: self.container,
            uri,
            range: to_lsp_range(source, self.range),
            selection_range: to_lsp_range(source, self.selection_range),
            data: Some(data),
        }
    }

    fn into_containing(self, source: &str) -> ContainingDeclaration {
        ContainingDeclaration {
            name: self.name,
            fqn: self.fqn,
            kind: self.kind,
            container: self.container,
            range: to_lsp_range(source, self.range),
            selection_range: to_lsp_range(source, self.selection_range),
        }
    }
}

fn kotlin_declaration_for_node(
    node: &SyntaxNode,
    package: Option<&str>,
    item_kind: SymbolKind,
) -> Option<KotlinDeclaration> {
    let declaration = KotlinDeclaration::from_node(node, package)?;
    if item_kind == SymbolKind::CONSTRUCTOR && node.kind() == K::CLASS_DECL {
        Some(declaration.into_constructor())
    } else {
        (declaration.kind == item_kind).then_some(declaration)
    }
}

/// Reconstruct the declaration named by opaque data and verify that none of
/// the client-visible identity fields were retargeted before expansion.
fn validate_kotlin_declaration(
    root: &SyntaxNode,
    source: &str,
    item: &CallHierarchyItem,
    expected_fqn: &str,
) -> Result<(SyntaxNode, KotlinDeclaration), String> {
    let encoded_range = encoded_kotlin_declaration_range(item)?;
    let package = package_of(root);
    let node = root
        .descendants()
        .find(|node| node.text_range() == encoded_range)
        .ok_or_else(|| "Kotlin call hierarchy declaration is unavailable".to_string())?;
    let declaration = kotlin_declaration_for_node(&node, package.as_deref(), item.kind)
        .ok_or_else(|| {
            "Kotlin call hierarchy item kind does not match its declaration".to_string()
        })?;
    if declaration.fqn != expected_fqn
        || declaration.name != item.name
        || declaration.container != item.detail
        || to_lsp_range(source, declaration.range) != item.range
        || to_lsp_range(source, declaration.selection_range) != item.selection_range
        || !declaration_identity_is_unique(root, &declaration)
    {
        return Err(
            "Kotlin call hierarchy item does not identify its exact declaration".to_string(),
        );
    }
    Ok((node, declaration))
}

#[derive(Debug)]
struct ConstructorParameterShape {
    name: String,
    has_default: bool,
    is_vararg: bool,
}

#[derive(Debug)]
struct ConstructorShape {
    declaration_range: TextRange,
    parameters: Vec<ConstructorParameterShape>,
}

fn constructor_parameter_shapes(list: &SyntaxNode) -> Vec<ConstructorParameterShape> {
    list.children()
        .filter(|node| node.kind() == K::PARAMETER)
        .filter_map(|parameter| {
            let name = parameter
                .children_with_tokens()
                .filter_map(|element| element.into_token())
                .find(|token| is_name_token(token.kind()))?
                .text()
                .to_string();
            let has_default = parameter
                .children_with_tokens()
                .any(|element| element.kind() == K::EQ);
            let is_vararg = parameter
                .descendants_with_tokens()
                .any(|element| element.kind() == K::VARARG_KW);
            Some(ConstructorParameterShape {
                name,
                has_default,
                is_vararg,
            })
        })
        .collect()
}

fn constructor_shape(node: &SyntaxNode) -> ConstructorShape {
    let parameters = node
        .children()
        .find(|child| child.kind() == K::PARAMETER_LIST)
        .map_or_else(Vec::new, |list| constructor_parameter_shapes(&list));
    ConstructorShape {
        declaration_range: node.text_range(),
        parameters,
    }
}

fn constructor_shapes_for_declaration(node: &SyntaxNode) -> Vec<ConstructorShape> {
    let owner = if node.kind() == K::CONSTRUCTOR_DECL {
        node.ancestors()
            .skip(1)
            .find(|ancestor| ancestor.kind() == K::CLASS_DECL)
    } else if node.kind() == K::CLASS_DECL {
        Some(node.clone())
    } else {
        None
    };
    let Some(owner) = owner else {
        return Vec::new();
    };
    let secondary: Vec<_> = owner
        .children()
        .filter(|child| child.kind() == K::CLASS_BODY)
        .flat_map(|body| body.children())
        .filter(|child| child.kind() == K::CONSTRUCTOR_DECL)
        .map(|constructor| constructor_shape(&constructor))
        .collect();

    let primary_parameters = owner
        .children()
        .find(|child| child.kind() == K::PARAMETER_LIST);
    let mut shapes = Vec::with_capacity(secondary.len() + 1);
    if let Some(parameters) = primary_parameters {
        shapes.push(ConstructorShape {
            declaration_range: owner.text_range(),
            parameters: constructor_parameter_shapes(&parameters),
        });
    } else if secondary.is_empty() {
        // A class with neither a written primary parameter list nor secondary
        // constructors has the implicit zero-argument primary constructor.
        shapes.push(ConstructorShape {
            declaration_range: owner.text_range(),
            parameters: Vec::new(),
        });
    }
    shapes.extend(secondary);
    shapes
}

fn constructor_shape_accepts_call(shape: &ConstructorShape, call: &KotlinCall) -> bool {
    // Trailing-lambda and partially typed calls need type-aware overload
    // resolution. They are deliberately unavailable rather than guessed.
    if call.trailing_lambda.is_some()
        || call.r_paren.is_none()
        || call.arg_count() != call.args.len()
        || shape.parameters.iter().any(|parameter| parameter.is_vararg)
    {
        return false;
    }

    let mut assigned = vec![false; shape.parameters.len()];
    let mut next_positional = 0;
    let mut saw_named = false;
    for argument in &call.args {
        let index = if let Some(name) = argument.name.as_deref() {
            saw_named = true;
            let Some(index) = shape
                .parameters
                .iter()
                .position(|parameter| parameter.name == name)
            else {
                return false;
            };
            index
        } else {
            if saw_named {
                return false;
            }
            while assigned.get(next_positional) == Some(&true) {
                next_positional += 1;
            }
            let index = next_positional;
            next_positional += 1;
            index
        };
        let Some(slot) = assigned.get_mut(index) else {
            return false;
        };
        if *slot {
            return false;
        }
        *slot = true;
    }
    assigned
        .iter()
        .zip(&shape.parameters)
        .all(|(assigned, parameter)| *assigned || parameter.has_default)
}

fn constructor_shape_accepts_java_arity(shape: &ConstructorShape, argument_count: usize) -> bool {
    // Kotlin default arguments do not make a shorter Java invocation legal
    // unless bytecode overloads are generated separately.  The source model
    // cannot prove that generation here, so require every non-vararg parameter.
    // A well-formed Kotlin constructor has at most one, final vararg.
    match shape
        .parameters
        .iter()
        .position(|parameter| parameter.is_vararg)
    {
        Some(vararg) if vararg + 1 == shape.parameters.len() => argument_count >= vararg,
        Some(_) => false,
        None => argument_count == shape.parameters.len(),
    }
}

fn unique_constructor_shape(
    shapes: &[ConstructorShape],
    mut accepts: impl FnMut(&ConstructorShape) -> bool,
) -> Option<&ConstructorShape> {
    let mut applicable = shapes.iter().filter(|shape| accepts(shape));
    let selected = applicable.next()?;
    applicable.next().is_none().then_some(selected)
}

fn constructor_call_at_usage(root: &SyntaxNode, range: TextRange) -> Option<KotlinCall> {
    let token = token_at(root, u32::from(range.start()))?;
    token.parent_ancestors().find_map(|node| {
        (node.kind() == K::CALL_EXPR)
            .then(|| analyze_call(&node))
            .flatten()
            .filter(|call| callee_name_range(call) == Some(range))
    })
}

fn java_constructor_argument_count_at_usage(
    root: &JavaSyntaxNode,
    range: TextRange,
) -> Option<usize> {
    root.descendants()
        .filter(|node| node.kind() == JavaKind::NEW_EXPR)
        .find_map(|new_expression| {
            let class_type = new_expression
                .children()
                .find(|child| child.kind() == JavaKind::CLASS_TYPE)?;
            if class_type.text_range() != range {
                return None;
            }
            let arguments = new_expression
                .children()
                .find(|child| child.kind() == JavaKind::ARG_LIST)?;
            arguments
                .children_with_tokens()
                .any(|element| element.kind() == JavaKind::R_PAREN)
                .then(|| crate::navigation::call_site_analysis::count_arguments(&arguments))
        })
}

/// The declaration a `prepare` request at `offset` should answer with.
///
/// The cursor has to be on a declaration-name token. Secondary constructors
/// are the one exception to Kotlin's identifier-only names: their selection
/// range is the `constructor` keyword itself.
fn declaration_at_offset(root: &SyntaxNode, offset: u32) -> Option<KotlinDeclaration> {
    let token = token_at(root, offset)?;
    if !is_hierarchy_name_token(&token) {
        return None;
    }
    let package = package_of(root);

    let mut node = token.parent()?;
    loop {
        match node.kind() {
            K::FUNCTION_DECL | K::CONSTRUCTOR_DECL | K::PROPERTY_ACCESSOR => {
                let declaration = KotlinDeclaration::from_node(&node, package.as_deref())?;
                return (declaration.selection_range == token.text_range()).then_some(declaration);
            }
            K::CLASS_DECL => {
                // Only the type's own name starts a constructor hierarchy; an
                // identifier elsewhere in the body is not a declaration.
                let declaration = KotlinDeclaration::from_node(&node, package.as_deref())?;
                return (declaration.selection_range == token.text_range())
                    .then(|| declaration.into_constructor());
            }
            K::OBJECT_DECL => return None,
            K::SOURCE_FILE => return None,
            _ => node = node.parent()?,
        }
    }
}

fn declaration_identity_is_unique(root: &SyntaxNode, declaration: &KotlinDeclaration) -> bool {
    if declaration.kind == SymbolKind::CONSTRUCTOR {
        // Every secondary constructor intentionally shares the owning type's
        // semantic FQN.  Count the owning type declarations, not the number of
        // constructor declarations, or any class with a secondary constructor
        // would look ambiguous.
        let package = package_of(root);
        return root
            .descendants()
            .filter(|node| node.kind() == K::CLASS_DECL)
            .filter_map(|node| KotlinDeclaration::from_node(&node, package.as_deref()))
            .filter(|candidate| candidate.fqn == declaration.fqn)
            .count()
            == 1;
    }
    kotlin_declarations_with_identity(root, &declaration.fqn, declaration.kind).len() == 1
}

fn kotlin_declarations_with_identity(
    root: &SyntaxNode,
    fqn: &str,
    kind: SymbolKind,
) -> Vec<KotlinDeclaration> {
    let package = package_of(root);
    root.descendants()
        .filter_map(|node| match kind {
            SymbolKind::FUNCTION | SymbolKind::METHOD if node.kind() == K::FUNCTION_DECL => {
                KotlinDeclaration::from_node(&node, package.as_deref())
            }
            SymbolKind::CONSTRUCTOR
                if matches!(node.kind(), K::CLASS_DECL | K::CONSTRUCTOR_DECL) =>
            {
                KotlinDeclaration::from_node(&node, package.as_deref()).map(|declaration| {
                    if node.kind() == K::CLASS_DECL {
                        declaration.into_constructor()
                    } else {
                        declaration
                    }
                })
            }
            SymbolKind::PROPERTY if node.kind() == K::PROPERTY_DECL => {
                KotlinDeclaration::from_node(&node, package.as_deref())
            }
            _ => None,
        })
        .filter(|candidate| candidate.fqn == fqn)
        .collect()
}

/// Collect calls owned by a declaration while excluding calls inside nested
/// declarations. A nested function/class is a separate hierarchy item, so its
/// calls must not be attributed to the enclosing function or constructor.
fn collect_call_exprs(node: &SyntaxNode, calls: &mut Vec<SyntaxNode>) {
    for child in node.children() {
        if matches!(
            child.kind(),
            K::FUNCTION_DECL
                | K::CONSTRUCTOR_DECL
                | K::PROPERTY_ACCESSOR
                | K::CLASS_DECL
                | K::OBJECT_DECL
                | K::INTERFACE_DECL
                | K::COMPANION_OBJECT
        ) {
            continue;
        }
        if child.kind() == K::CALL_EXPR {
            calls.push(child.clone());
        }
        collect_call_exprs(&child, calls);
    }
}

// ---------------------------------------------------------------------------
// CST readers
// ---------------------------------------------------------------------------

/// Token at `offset`, preferring an identifier over adjacent punctuation.
fn token_at(root: &SyntaxNode, offset: u32) -> Option<SyntaxToken> {
    let clamped = TextSize::from(offset).min(root.text_range().end());
    let at_offset = root.token_at_offset(clamped);
    let right = at_offset.clone().right_biased();
    if right.as_ref().is_some_and(is_hierarchy_name_token) {
        return right;
    }
    at_offset
        .left_biased()
        .filter(is_hierarchy_name_token)
        .or(right)
}

fn is_hierarchy_name_token(token: &SyntaxToken) -> bool {
    is_name_token(token.kind()) || token.kind() == K::CONSTRUCTOR_KW
}

/// Package of a `SOURCE_FILE`, if it declares one.
pub(crate) fn package_of(root: &SyntaxNode) -> Option<String> {
    let declaration = root
        .children()
        .find(|child| child.kind() == K::PACKAGE_DECL)?;
    let parts: Vec<String> = declaration
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == K::IDENT)
        .map(|token| token.text().to_string())
        .collect();
    (!parts.is_empty()).then(|| parts.join("."))
}

/// Simple names of the type declarations enclosing `node`, outermost first.
fn enclosing_type_chain(node: &SyntaxNode) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            K::CLASS_DECL | K::OBJECT_DECL | K::INTERFACE_DECL | K::COMPANION_OBJECT
        ) {
            names.push(
                type_name_token(&parent)
                    .map_or_else(|| "Companion".to_string(), |token| token.text().to_string()),
            );
        }
        current = parent.parent();
    }
    names.reverse();
    names
}

fn join_fqn(package: Option<&str>, owners: &[String], name: &str) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(owners.len() + 2);
    if let Some(package) = package {
        parts.push(package);
    }
    parts.extend(owners.iter().map(String::as_str));
    parts.push(name);
    parts.join(".")
}

/// Name token of a `FUNCTION_DECL` — the last identifier before the parameter
/// list, so `fun <T> String.trimTo(n: Int)` yields `trimTo`, not `String`.
fn function_name_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut name = None;
    for child in node.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Node(n) if n.kind() == K::PARAMETER_LIST => break,
            rowan::NodeOrToken::Token(token) if is_name_token(token.kind()) => {
                name = Some(token.clone());
            }
            _ => {}
        }
    }
    name
}

/// Name token of a `CLASS_DECL` / `OBJECT_DECL` / `INTERFACE_DECL`.
fn type_name_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|child| child.into_token())
        .find(|token| is_name_token(token.kind()))
}

/// Name token of a `PROPERTY_DECL`: the identifier after `val` / `var`.
fn property_name_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut seen_keyword = false;
    for child in node.children_with_tokens() {
        let Some(token) = child.into_token() else {
            continue;
        };
        if matches!(token.kind(), K::VAL_KW | K::VAR_KW) {
            seen_keyword = true;
        } else if seen_keyword && is_name_token(token.kind()) {
            return Some(token);
        }
    }
    None
}

/// Name of the property an accessor belongs to.
fn accessor_property(accessor: &SyntaxNode) -> Option<String> {
    let mut current = accessor.parent();
    while let Some(node) = current {
        if node.kind() == K::PROPERTY_DECL {
            return property_name_token(&node).map(|token| token.text().to_string());
        }
        current = node.parent();
    }
    None
}

/// Span of the callee's name inside a call.
///
/// This is the span the usage extractor records for the same call, which is what
/// keeps outgoing ranges and incoming ranges describing the same thing.
fn callee_name_range(call: &KotlinCall) -> Option<rowan::TextRange> {
    let callee = call.node.children_with_tokens().find(|child| {
        !child.kind().is_trivia() && !matches!(child.kind(), K::L_PAREN | K::TYPE_ARGUMENT_LIST)
    })?;

    match callee {
        // `a.b(…)` / `a?.b(…)`: the name is the access node's last identifier.
        rowan::NodeOrToken::Node(node)
            if matches!(node.kind(), K::MEMBER_ACCESS_EXPR | K::SAFE_ACCESS_EXPR) =>
        {
            node.children_with_tokens()
                .filter_map(|child| child.into_token())
                .filter(|token| is_name_token(token.kind()))
                .last()
                .map(|token| token.text_range())
        }
        rowan::NodeOrToken::Token(token) if is_name_token(token.kind()) => Some(token.text_range()),
        _ => None,
    }
}

fn to_lsp_range(source: &str, range: rowan::TextRange) -> Range {
    Range {
        start: offset_to_position(source, usize::from(range.start())),
        end: offset_to_position(source, usize::from(range.end())),
    }
}

/// Kinds whose name doubles as a constructor call.
const fn is_type(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::CLASS | SymbolKind::OBJECT)
}

/// Identity of a caller, so repeated call sites in one declaration merge.
type CallerKey = (String, Url, u32, u32, u32, u32);

fn caller_key(item: &CallHierarchyItem) -> CallerKey {
    (
        item.name.clone(),
        item.uri.clone(),
        item.range.start.line,
        item.range.start.character,
        item.range.end.line,
        item.range.end.character,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position_of(source: &str, needle: &str) -> Position {
        offset_to_position(source, source.find(needle).expect("fixture marker"))
    }

    #[test]
    fn kotlin_prepare_fails_closed_for_descriptorless_overloads() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Subject.kt");
        let source = concat!(
            "package p\nclass Subject { ",
            "fun pick(value: Int) {} ",
            "fun pick(value: String) {} }",
        );
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&path, source)
            .expect("index Kotlin fixture");
        let file_id = index.get_file_id(&path).unwrap();
        let host = crate::semantic_host::SemanticHost::new(Arc::clone(&index));
        let txn = host.read_now();
        let provider = KotlinCallHierarchyProvider::new(&txn, Arc::clone(&index));
        let uri = Url::from_file_path(&path).unwrap();

        assert!(provider
            .prepare(
                file_id,
                position_of(source, "pick(value: Int"),
                source,
                &uri
            )
            .expect("prepare result")
            .is_none());
        assert!(provider
            .prepare(
                file_id,
                position_of(source, "pick(value: String"),
                source,
                &uri,
            )
            .expect("prepare result")
            .is_none());
    }

    #[test]
    fn kotlin_same_text_republication_retires_a_prepared_item() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("Subject.kt");
        let source = "package p\nclass Subject { fun run() {} }";
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&path, source)
            .expect("Kotlin R1");
        let file_id = index.get_file_id(&path).unwrap();
        let host = crate::semantic_host::SemanticHost::new(Arc::clone(&index));
        let txn = host.read_now();
        let provider = KotlinCallHierarchyProvider::new(&txn, Arc::clone(&index));
        let uri = Url::from_file_path(&path).unwrap();
        let item = provider
            .prepare(file_id, position_of(source, "run()"), source, &uri)
            .unwrap()
            .unwrap()
            .remove(0);

        index
            .index_file_with_source(&path, source)
            .expect("Kotlin same-text R2");
        assert!(provider.incoming_calls(&item).is_err());
        assert!(provider.outgoing_calls(&item).is_err());
    }
}
