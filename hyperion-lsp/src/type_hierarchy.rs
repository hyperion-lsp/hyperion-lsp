// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type Hierarchy Provider
//!
//! Implements LSP type hierarchy for navigating class inheritance and interface implementations.
//! Supports "Go to Super Types" and "Go to Implementations" features.

use hyperion_parser::syntax::SyntaxKind;
use hyperion_semantics::db::FileId;
use hyperion_workspace::{
    DeclarationNamespace, DefinitionSourceAuthority, FileRevisionToken, ResolvedDefinition, Symbol,
    SymbolTarget, TypeNameResolution, WorkspaceIndex, WorkspaceRevision,
};
use std::num::NonZeroU64;
use std::sync::Arc;
use tower_lsp::lsp_types::{Position, Range, SymbolKind, TypeHierarchyItem, Url};
use tracing::{debug, warn};

use crate::navigation_util::{offset_to_position, position_to_offset};
use crate::source_provider::SourceProviderChain;

const DATA_FQN: &str = "fqn";
const DATA_WORKSPACE_REVISION: &str = "w";
const DATA_FILE_ID: &str = "i";
const DATA_FILE_REVISION: &str = "r";
const DATA_EXTERNAL_FILE_ID: &str = "e";

#[derive(Debug, Clone, PartialEq, Eq)]
enum HierarchyDeclarationProvenance {
    Workspace(FileRevisionToken),
    // External FileIds are allocated monotonically from a disjoint range and
    // are never reused. Together with the workspace revision, the ID is the
    // compact exact identity of this declaration for the session.
    External(FileId),
}

/// Exact declaration and inheritance-graph identity carried by an LSP item.
///
/// The wire representation uses decimal strings for every integer: JSON
/// numbers cannot exactly represent all non-zero `u64` revision IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HierarchyItemProvenance {
    fqn: Arc<str>,
    workspace_revision: WorkspaceRevision,
    declaration: HierarchyDeclarationProvenance,
}

impl HierarchyItemProvenance {
    pub(crate) fn capture(index: &WorkspaceIndex, fqn: &str) -> Option<(Self, ResolvedDefinition)> {
        let target = SymbolTarget::new(fqn, DeclarationNamespace::Type);
        for _ in 0..2 {
            // The typed query elects the type namespace and stamps its current
            // source authority under one index read. In particular, an
            // identical didOpen adoption gets the newly minted file token even
            // though the reusable symbol/range product was not rebuilt.
            let definition = index.find_symbol_definition(&target)?;
            let declaration = match definition.source {
                DefinitionSourceAuthority::External => {
                    HierarchyDeclarationProvenance::External(definition.location.file_id)
                }
                DefinitionSourceAuthority::Workspace(revision) => {
                    HierarchyDeclarationProvenance::Workspace(revision)
                }
            };
            let provenance = Self {
                fqn: Arc::from(fqn),
                workspace_revision: definition.workspace_revision,
                declaration,
            };
            if provenance.is_current(index) {
                return Some((provenance, definition));
            }
        }
        None
    }

    pub(crate) fn decode_current(index: &WorkspaceIndex, data: &serde_json::Value) -> Option<Self> {
        let fqn: Arc<str> = Arc::from(data.get(DATA_FQN)?.as_str()?);
        let encoded_workspace = parse_non_zero(data.get(DATA_WORKSPACE_REVISION)?.as_str()?)?;
        let workspace_revision = index.current_workspace_revision();
        if workspace_revision.get() != encoded_workspace {
            return None;
        }

        let declaration = match (
            data.get(DATA_EXTERNAL_FILE_ID),
            data.get(DATA_FILE_ID),
            data.get(DATA_FILE_REVISION),
        ) {
            (Some(source), None, None) => {
                let file_id = FileId::new(source.as_str()?.parse().ok()?);
                index.get_external_source_identity(file_id)?;
                HierarchyDeclarationProvenance::External(file_id)
            }
            (None, Some(file_id), Some(revision_id)) => {
                let file_id = FileId::new(file_id.as_str()?.parse().ok()?);
                let revision_id = parse_non_zero(revision_id.as_str()?)?;
                let revision = index.current_file_revision(file_id)?;
                if revision.revision_id() != revision_id {
                    return None;
                }
                HierarchyDeclarationProvenance::Workspace(revision)
            }
            _ => return None,
        };
        let provenance = Self {
            fqn,
            workspace_revision,
            declaration,
        };
        provenance.is_current(index).then_some(provenance)
    }

    pub(crate) fn encode(&self) -> serde_json::Value {
        match &self.declaration {
            HierarchyDeclarationProvenance::Workspace(revision) => serde_json::json!({
                "fqn": self.fqn.as_ref(),
                "w": self.workspace_revision.get().to_string(),
                "i": revision.file_id().as_u32().to_string(),
                "r": revision.revision_id().to_string(),
            }),
            HierarchyDeclarationProvenance::External(file_id) => serde_json::json!({
                "fqn": self.fqn.as_ref(),
                "w": self.workspace_revision.get().to_string(),
                "e": file_id.as_u32().to_string(),
            }),
        }
    }

    pub(crate) fn fqn(&self) -> &str {
        &self.fqn
    }

    pub(crate) const fn workspace_file_revision(&self) -> Option<FileRevisionToken> {
        match self.declaration {
            HierarchyDeclarationProvenance::Workspace(revision) => Some(revision),
            HierarchyDeclarationProvenance::External(_) => None,
        }
    }

    pub(crate) fn is_current(&self, index: &WorkspaceIndex) -> bool {
        let target = SymbolTarget::new(Arc::clone(&self.fqn), DeclarationNamespace::Type);
        let Some(current) = index.find_symbol_definition(&target) else {
            return false;
        };
        if current.workspace_revision != self.workspace_revision {
            return false;
        }
        match &self.declaration {
            HierarchyDeclarationProvenance::Workspace(revision) => matches!(
                current.source,
                DefinitionSourceAuthority::Workspace(current_revision)
                    if current_revision == *revision
            ),
            HierarchyDeclarationProvenance::External(file_id) => {
                if current.source != DefinitionSourceAuthority::External {
                    return false;
                }
                current.location.file_id == *file_id
                    && index.get_external_source_identity(*file_id).is_some()
                    && index.current_workspace_revision() == self.workspace_revision
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HierarchyRequestProvenance {
    workspace_revision: WorkspaceRevision,
    file_revision: FileRevisionToken,
}

impl HierarchyRequestProvenance {
    pub(crate) fn capture(
        index: &WorkspaceIndex,
        file_id: FileId,
        source_text: &str,
    ) -> Option<Self> {
        let workspace_revision = index.current_workspace_revision();
        let file_revision = index.current_file_revision(file_id)?;
        let indexed = index.get_indexed_revision_content(file_revision)?;
        if indexed.content.as_str() != source_text {
            return None;
        }
        let provenance = Self {
            workspace_revision,
            file_revision,
        };
        provenance.is_current(index).then_some(provenance)
    }

    pub(crate) fn is_current(self, index: &WorkspaceIndex) -> bool {
        index.workspace_revisions_are_current(
            self.workspace_revision,
            std::slice::from_ref(&self.file_revision),
        )
    }
}

fn parse_non_zero(value: &str) -> Option<NonZeroU64> {
    NonZeroU64::new(value.parse().ok()?)
}

/// Provides type hierarchy information for classes and interfaces
pub struct TypeHierarchyProvider {
    workspace_index: Arc<WorkspaceIndex>,
    source_providers: Arc<SourceProviderChain>,
}

impl TypeHierarchyProvider {
    /// Create a new TypeHierarchyProvider
    pub const fn new(
        workspace_index: Arc<WorkspaceIndex>,
        source_providers: Arc<SourceProviderChain>,
    ) -> Self {
        Self {
            workspace_index,
            source_providers,
        }
    }

    fn with_stable_workspace<T>(
        &self,
        operation: impl FnOnce() -> Result<Option<T>, String>,
    ) -> Result<Option<T>, String> {
        let revision = self.workspace_index.current_workspace_revision();
        let result = operation()?;
        if self.workspace_index.current_workspace_revision() != revision {
            debug!("Type hierarchy: workspace bindings changed during request");
            return Ok(None);
        }
        Ok(result)
    }

    /// Prepare type hierarchy - find the class/interface at the cursor position
    ///
    /// This is the entry point for type hierarchy. It returns a TypeHierarchyItem
    /// representing the type at the cursor position.
    pub fn prepare_type_hierarchy(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        let Some(request) =
            HierarchyRequestProvenance::capture(&self.workspace_index, file_id, source_text)
        else {
            debug!("Type hierarchy: request bytes have no exact indexed revision");
            return Ok(None);
        };
        let result = self.prepare_type_hierarchy_at_revision(file_id, position, source_text)?;
        if !request.is_current(&self.workspace_index) {
            debug!("Type hierarchy: request source or graph changed during prepare");
            return Ok(None);
        }
        Ok(result)
    }

    fn prepare_type_hierarchy_at_revision(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        debug!(
            "Preparing type hierarchy at position {:?} in file {:?}",
            position, file_id
        );

        // Convert position to offset
        let offset = position_to_offset(source_text, position)
            .map_err(|e| format!("Failed to convert position: {e}"))?;

        // Parse the exact request bytes. A shared Salsa tree may already belong
        // to a newer document publication even while this request still holds
        // the previous editor buffer.
        let tokens = hyperion_parser::Lexer::new(source_text).collect();
        let (green, _errors) = hyperion_parser::Parser::parse(tokens);
        let root = hyperion_parser::SyntaxNode::new_root(green);

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

        if token.kind() != SyntaxKind::IDENT {
            return Ok(None);
        }

        // A declaration name must win over global simple-name lookup. Doing
        // reference resolution first made `alpha.Derived` select an arbitrary
        // `Derived` from another package.
        let declaration = token.parent().and_then(|parent| {
            parent.ancestors().find(|node| {
                node.kind().is_type_declaration()
                    && node
                        .children_with_tokens()
                        .find(|child| child.kind() == SyntaxKind::IDENT)
                        .is_some_and(|name| name.text_range() == token.text_range())
            })
        });
        let Some(node) = declaration else {
            return self.resolve_type_reference_at_cursor(file_id, &token);
        };

        let type_name = self.extract_type_name(&node)?;
        let Some(file_path) = self.workspace_index.get_file_path(file_id) else {
            warn!("Could not get file path for file_id {:?}", file_id);
            return Ok(None);
        };
        let uri =
            Url::from_file_path(&file_path).map_err(|()| "Failed to create URI from file path")?;

        let text_range = node.text_range();
        let range = Range {
            start: offset_to_position(source_text, text_range.start().into()),
            end: offset_to_position(source_text, text_range.end().into()),
        };
        let name_range = token.text_range();
        let selection_range = Range {
            start: offset_to_position(source_text, name_range.start().into()),
            end: offset_to_position(source_text, name_range.end().into()),
        };
        let kind = match node.kind() {
            SyntaxKind::CLASS_DECL => SymbolKind::CLASS,
            SyntaxKind::ENUM_DECL => SymbolKind::ENUM,
            // Records are indexed as STRUCT (symbol_extractor.rs)
            SyntaxKind::RECORD_DECL => SymbolKind::STRUCT,
            _ => SymbolKind::INTERFACE,
        };
        let Some(fqn) = self
            .type_symbol_in_file(file_id, &type_name, Some(offset))
            .map(|symbol| symbol.fqn.to_string())
        else {
            return Ok(None);
        };
        let Some((provenance, definition)) =
            HierarchyItemProvenance::capture(&self.workspace_index, &fqn)
        else {
            return Ok(None);
        };
        if definition.location.file_id != file_id
            || provenance.workspace_file_revision()
                != self.workspace_index.current_file_revision(file_id)
        {
            return Ok(None);
        }

        Ok(Some(vec![TypeHierarchyItem {
            name: type_name,
            kind,
            tags: None,
            detail: Some(fqn),
            uri,
            range,
            selection_range,
            data: Some(provenance.encode()),
        }]))
    }

    /// Try to resolve a token as a type reference and create a TypeHierarchyItem
    fn resolve_type_reference_at_cursor(
        &self,
        file_id: FileId,
        token: &hyperion_parser::syntax::SyntaxToken,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        // A matching type name is not sufficient: fields, methods, and locals
        // may legally have the same spelling as a type. Restrict this fallback
        // to CST positions that actually denote a Java type.
        if token.kind() != SyntaxKind::IDENT
            || !token.parent_ancestors().any(|node| {
                node.kind() == SyntaxKind::CLASS_TYPE
                    || (node.kind() == SyntaxKind::ANNOTATION
                        && node
                            .descendants_with_tokens()
                            .filter_map(|element| element.into_token())
                            .find(|candidate| candidate.kind() == SyntaxKind::IDENT)
                            .is_some_and(|name| name.text_range() == token.text_range()))
            })
        {
            return Ok(None);
        }

        let type_name = java_type_reference_name(token).unwrap_or_else(|| token.text().to_string());
        debug!("Trying to resolve type reference: {}", type_name);

        // Type hierarchy must never invent an inheritance edge by choosing the
        // first of two legal-looking import candidates. Resolve according to
        // lexical/package/import precedence, but require on-demand and global
        // lookup to be unambiguous.
        if let Some(fqn) = self.resolve_type_reference_fqn(file_id, token, &type_name) {
            return self
                .create_type_hierarchy_item(&fqn)
                .map(|item| item.map(|item| vec![item]));
        }

        // Java has no workspace-wide implicit import. Even a unique global
        // simple-name match is not visible here unless its package/import path
        // resolved above.
        debug!("Could not resolve type reference: {}", type_name);
        Ok(None)
    }

    fn resolve_type_reference_fqn(
        &self,
        file_id: FileId,
        token: &hyperion_parser::syntax::SyntaxToken,
        name: &str,
    ) -> Option<String> {
        let name = erase_type_arguments(name);
        let name = name.trim();
        if name.is_empty() {
            return None;
        }

        let imports = self.workspace_index.get_imports(file_id);
        let package = imports
            .as_ref()
            .and_then(|imports| imports.current_package.as_deref());

        // The CST owns the source-scope question (which declarations actually
        // enclose this token); the workspace owns Java namespace semantics.
        // Pass fully-qualified owners nearest-first to the canonical resolver.
        let simple_owners = java_type_scope_chain(token);
        let lexical_owners: Vec<_> = (1..=simple_owners.len())
            .rev()
            .map(|depth| {
                let owner = simple_owners[..depth].join(".");
                package.map_or(owner.clone(), |package| format!("{package}.{owner}"))
            })
            .collect();

        match self
            .workspace_index
            .resolve_type_name(file_id, &lexical_owners, name)
        {
            TypeNameResolution::Exact(fqn) => Some(fqn),
            TypeNameResolution::Missing | TypeNameResolution::Ambiguous(_) => None,
        }
    }

    /// Get supertypes (parent class and implemented interfaces) for a type
    pub fn supertypes(
        &self,
        item: &TypeHierarchyItem,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        self.with_stable_workspace(|| self.supertypes_at_revision(item))
    }

    fn supertypes_at_revision(
        &self,
        item: &TypeHierarchyItem,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        debug!("Getting supertypes for {}", item.name);

        let Some(symbol) = self.type_symbol_for_item(item) else {
            debug!("Symbol not found: {}", item.name);
            return Ok(None);
        };

        let mut supertypes = Vec::new();
        let mut has_superclass = false;
        for supertype_fqn in self.workspace_index.direct_supertypes(&symbol.fqn) {
            debug!("Found direct supertype: {supertype_fqn}");
            has_superclass |= self
                .workspace_index
                .get_symbol(&supertype_fqn)
                .is_some_and(|supertype| supertype.kind == SymbolKind::CLASS);
            if let Some(item) = self.create_supertype_item(&supertype_fqn)? {
                supertypes.push(item);
            }
        }
        // Every class without an explicit superclass extends java.lang.Object
        // (interfaces, enums and records have their own implicit roots).
        if symbol.kind == SymbolKind::CLASS
            && !has_superclass
            && symbol.fqn.as_ref() != "java.lang.Object"
        {
            if let Some(object) = self.create_supertype_item("java.lang.Object")? {
                supertypes.push(object);
            }
        }

        if supertypes.is_empty() {
            Ok(None)
        } else {
            Ok(Some(supertypes))
        }
    }

    /// Get direct subtypes (classes or interfaces that immediately extend or
    /// implement this type).
    pub fn subtypes(
        &self,
        item: &TypeHierarchyItem,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        self.with_stable_workspace(|| self.descendant_items(item, false))
    }

    /// Get all transitive, concrete implementations for goto-implementation.
    ///
    /// This deliberately differs from `subtypes`: the LSP type-hierarchy
    /// endpoint is one level at a time, while goto-implementation must walk
    /// through subinterfaces and abstract classes to reach usable targets.
    pub fn implementations(
        &self,
        item: &TypeHierarchyItem,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        self.with_stable_workspace(|| self.descendant_items(item, true))
    }

    fn descendant_items(
        &self,
        item: &TypeHierarchyItem,
        transitive: bool,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        let relation = if transitive {
            "implementations"
        } else {
            "direct subtypes"
        };
        debug!("Getting {relation} for {}", item.name);

        let Some(symbol) = self.type_symbol_for_item(item) else {
            debug!("Symbol not found: {}", item.name);
            return Ok(None);
        };

        let target_fqn = &symbol.fqn;
        debug!(
            "Looking for {relation} of {} (FQN: {})",
            item.name, target_fqn,
        );

        let descendants = if transitive {
            self.workspace_index.find_implementations(target_fqn)
        } else {
            self.workspace_index.find_direct_subtypes(target_fqn)
        };
        debug!(
            "Inheritance query returned {} {relation} result(s) for {}",
            descendants.len(),
            target_fqn,
        );

        if descendants.is_empty() {
            debug!("No {relation} found for {target_fqn}");
            return Ok(None);
        }

        debug!("Found {} {relation} of {}", descendants.len(), target_fqn,);

        let items: Vec<TypeHierarchyItem> = descendants
            .iter()
            .filter_map(
                |descendant| match self.create_type_hierarchy_item(&descendant.fqn) {
                    Ok(Some(item)) => Some(item),
                    Ok(None) => {
                        debug!("Could not create TypeHierarchyItem for {}", descendant.fqn);
                        None
                    }
                    Err(e) => {
                        warn!(
                            "Error creating TypeHierarchyItem for {}: {}",
                            descendant.fqn, e
                        );
                        None
                    }
                },
            )
            .collect();

        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(items))
        }
    }

    /// Extract the type name from a CLASS_DECL or INTERFACE_DECL node
    fn extract_type_name(
        &self,
        node: &hyperion_parser::syntax::SyntaxNode,
    ) -> Result<String, String> {
        // Look for IDENT token in children
        for child in node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = child {
                if token.kind() == SyntaxKind::IDENT {
                    return Ok(token.text().to_string());
                }
            }
        }
        Err("Could not find type name".to_string())
    }

    /// Resolve an item only while its exact declaration and graph proof remain
    /// current. Hierarchy items are opaque server data echoed by the client;
    /// missing/legacy data cannot safely be rebound to today's workspace.
    fn type_symbol_for_item(&self, item: &TypeHierarchyItem) -> Option<Arc<Symbol>> {
        let provenance =
            HierarchyItemProvenance::decode_current(&self.workspace_index, item.data.as_ref()?)?;
        self.workspace_index
            .get_symbol_target(&SymbolTarget::new(
                Arc::clone(&provenance.fqn),
                DeclarationNamespace::Type,
            ))
            .filter(|symbol| Self::is_type_symbol(symbol))
    }

    fn type_symbol_in_file(
        &self,
        file_id: FileId,
        name: &str,
        offset: Option<u32>,
    ) -> Option<Arc<Symbol>> {
        [
            SymbolKind::CLASS,
            SymbolKind::INTERFACE,
            SymbolKind::ENUM,
            SymbolKind::STRUCT,
        ]
        .into_iter()
        .find_map(|kind| {
            self.workspace_index
                .find_symbol_in_file(file_id, name, kind, offset)
        })
    }

    const fn is_type_symbol(symbol: &Symbol) -> bool {
        // ENUM and STRUCT (records) participate in type hierarchies too:
        // `enum Color implements Printable` is a subtype of Printable
        // (the Kotlin provider already includes ENUM/OBJECT).
        matches!(
            symbol.kind,
            SymbolKind::CLASS | SymbolKind::INTERFACE | SymbolKind::ENUM | SymbolKind::STRUCT
        )
    }

    /// Create a TypeHierarchyItem from an FQN by looking it up in the workspace
    /// A supertype item that survives missing source: a class's supertypes
    /// must list `java.lang.Object` (and library supertypes) even when no
    /// `src.zip` or source jar provides a navigable location. Such items carry
    /// a virtual `jdk:` / `dependency:` URI with an empty range; only the
    /// supertype view uses this, subtype and implementation queries keep
    /// requiring real source (a JDK type has thousands of subtypes).
    fn create_supertype_item(&self, fqn: &str) -> Result<Option<TypeHierarchyItem>, String> {
        if let Some(item) = self.create_type_hierarchy_item(fqn)? {
            return Ok(Some(item));
        }
        let Some((provenance, definition)) =
            HierarchyItemProvenance::capture(&self.workspace_index, fqn)
        else {
            return Ok(None);
        };
        if !self
            .workspace_index
            .is_external_file(definition.location.file_id)
        {
            return Ok(None);
        }
        let scheme = if self
            .workspace_index
            .get_external_source_name(definition.location.file_id)
            .as_deref()
            == Some("jdk")
        {
            "jdk"
        } else {
            "dependency"
        };
        let Ok(uri) = Url::parse(&format!("{scheme}:///{fqn}")) else {
            return Ok(None);
        };
        if !provenance.is_current(&self.workspace_index) {
            return Ok(None);
        }
        let empty = Range::new(Position::new(0, 0), Position::new(0, 0));
        Ok(Some(TypeHierarchyItem {
            name: definition.symbol.simple_name.to_string(),
            kind: definition.symbol.kind,
            tags: None,
            detail: Some(fqn.to_string()),
            uri,
            range: empty,
            selection_range: empty,
            data: Some(provenance.encode()),
        }))
    }

    fn create_type_hierarchy_item(&self, fqn: &str) -> Result<Option<TypeHierarchyItem>, String> {
        let Some((provenance, definition)) =
            HierarchyItemProvenance::capture(&self.workspace_index, fqn)
        else {
            warn!("Could not capture exact hierarchy provenance for: {fqn}");
            return Ok(None);
        };
        // Read presentation metadata after capturing the proof. The final
        // fence below rejects a concurrent replacement, so the returned item
        // can never combine an old symbol with a new declaration identity.
        let symbol = &definition.symbol;

        // Workspace files and attached/decompiled dependency sources share one
        // location conversion path. In particular, external FileIds have no
        // workspace path; their source provider owns the stable temp-file URI.
        let Some(location) = crate::navigation_util::symbol_location_to_lsp_location(
            &self.workspace_index,
            &definition.location,
            Some(&self.source_providers),
        )?
        else {
            debug!("No navigable source for hierarchy item: {fqn}");
            return Ok(None);
        };
        if !provenance.is_current(&self.workspace_index) {
            return Ok(None);
        }

        Ok(Some(TypeHierarchyItem {
            name: symbol.simple_name.to_string(),
            kind: symbol.kind,
            tags: None,
            detail: Some(fqn.to_string()), // Show FQN as detail
            uri: location.uri,
            range: location.range,
            selection_range: location.range,
            data: Some(provenance.encode()),
        }))
    }
}

fn java_type_reference_name(token: &hyperion_parser::syntax::SyntaxToken) -> Option<String> {
    let class_type = token
        .parent_ancestors()
        .find(|node| node.kind() == SyntaxKind::CLASS_TYPE)?;

    let mut name = String::new();
    let mut type_argument_depth = 0_u32;
    for element in class_type.descendants_with_tokens() {
        let Some(token) = element.into_token() else {
            continue;
        };
        if token
            .parent_ancestors()
            .take_while(|node| node != &class_type)
            .any(|node| node.kind() == SyntaxKind::ANNOTATION)
        {
            continue;
        }
        match token.kind() {
            SyntaxKind::LT => type_argument_depth += 1,
            SyntaxKind::GT => type_argument_depth = type_argument_depth.saturating_sub(1),
            SyntaxKind::SHR => type_argument_depth = type_argument_depth.saturating_sub(2),
            SyntaxKind::USHR => type_argument_depth = type_argument_depth.saturating_sub(3),
            SyntaxKind::IDENT | SyntaxKind::DOT if type_argument_depth == 0 => {
                name.push_str(token.text());
            }
            _ => {}
        }
    }
    (!name.is_empty()).then_some(name)
}

fn java_type_scope_chain(token: &hyperion_parser::syntax::SyntaxToken) -> Vec<String> {
    let mut owners: Vec<_> = token
        .parent_ancestors()
        .filter(|node| node.kind().is_type_declaration())
        .filter(|node| {
            if node.kind() == SyntaxKind::RECORD_DECL {
                // JLS 6.3: a record member's scope is the entire record
                // declaration, including the component/header region.
                return true;
            }
            // A declaration's member types enter scope in its body, not in its
            // own extends/implements header. For an outer declaration this
            // token is already after the outer body's opening brace, so it is
            // retained. The body brace is a direct declaration child in this
            // CST; descendant braces may belong to header annotations.
            node.children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .find(|candidate| candidate.kind() == SyntaxKind::L_BRACE)
                .is_some_and(|body_start| {
                    body_start.text_range().start() < token.text_range().start()
                })
        })
        .filter_map(|node| {
            node.children_with_tokens()
                .filter_map(rowan::NodeOrToken::into_token)
                .find(|token| token.kind() == SyntaxKind::IDENT)
                .map(|token| token.text().to_string())
        })
        .collect();
    owners.reverse();
    owners
}

fn erase_type_arguments(name: &str) -> String {
    let mut erased = String::with_capacity(name.len());
    let mut depth = 0_u32;
    for character in name.chars() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => erased.push(character),
            _ => {}
        }
    }
    erased
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_provider::{
        ExternalSourceRequest, SourceContent, SourceFile, SourceProvider,
    };
    use hyperion_workspace::{Modifiers, TextRange};
    use std::path::PathBuf;

    #[derive(Clone)]
    struct StaticSourceProvider {
        contents: std::collections::HashMap<Arc<str>, SourceContent>,
    }

    impl SourceProvider for StaticSourceProvider {
        fn get_source_exact(&self, request: ExternalSourceRequest<'_>) -> Option<SourceContent> {
            (request.source_identity()
                == &hyperion_workspace::ExternalSourceIdentity::for_virtual(
                    crate::source_provider::jdk::JDK_SOURCE_ID,
                ))
                .then(|| self.contents.get(request.fqn()).cloned())
                .flatten()
        }

        fn can_provide_exact(&self, request: ExternalSourceRequest<'_>) -> bool {
            request.source_identity()
                == &hyperion_workspace::ExternalSourceIdentity::for_virtual(
                    crate::source_provider::jdk::JDK_SOURCE_ID,
                )
                && self.contents.contains_key(request.fqn())
        }

        fn owns_source_identity(
            &self,
            source_identity: &hyperion_workspace::ExternalSourceIdentity,
        ) -> bool {
            source_identity
                == &hyperion_workspace::ExternalSourceIdentity::for_virtual(
                    crate::source_provider::jdk::JDK_SOURCE_ID,
                )
        }

        fn snapshot_for_sources(
            &self,
            source_identities: &std::collections::HashSet<
                hyperion_workspace::ExternalSourceIdentity,
            >,
        ) -> Option<Arc<dyn SourceProvider>> {
            source_identities
                .iter()
                .any(|identity| self.owns_source_identity(identity))
                .then(|| Arc::new(self.clone()) as Arc<dyn SourceProvider>)
        }

        fn priority(&self) -> u32 {
            0
        }

        fn name(&self) -> &'static str {
            "type-hierarchy-test"
        }
    }

    fn type_symbol(fqn: &str, parent_class: Option<&str>) -> Symbol {
        Symbol {
            fqn: fqn.into(),
            simple_name: fqn.rsplit('.').next().unwrap().into(),
            range: TextRange::new(0, 0),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers::default(),
            container: None,
            return_type: None,
            parameters: Vec::new(),
            parent_class: parent_class.map(Into::into),
            interfaces: Vec::new(),
            documentation: None,
            type_parameters: Vec::new(),
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        }
    }

    #[test]
    fn hierarchy_provenance_is_compact_and_uses_string_revision_ids() {
        assert!(std::mem::size_of::<HierarchyItemProvenance>() <= 48);
        assert!(std::mem::size_of::<HierarchyRequestProvenance>() <= 24);

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("p/Target.java");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = "package p; class Target {}";
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index.index_file_with_source(&path, source).unwrap();
        let (provenance, _) = HierarchyItemProvenance::capture(&index, "p.Target").unwrap();
        let data = provenance.encode();

        assert_eq!(
            data.get("fqn").and_then(serde_json::Value::as_str),
            Some("p.Target")
        );
        for key in ["w", "i", "r"] {
            let encoded = data
                .get(key)
                .and_then(serde_json::Value::as_str)
                .expect("revision components are JSON strings");
            assert!(encoded.parse::<u64>().is_ok());
        }
        assert_eq!(
            HierarchyItemProvenance::decode_current(&index, &data),
            Some(provenance)
        );

        let mut numeric_revision = data;
        numeric_revision["r"] = serde_json::json!(1);
        assert!(HierarchyItemProvenance::decode_current(&index, &numeric_revision).is_none());
    }

    #[test]
    fn same_text_republication_rejects_java_hierarchy_items_and_graph_queries() {
        let temp = tempfile::tempdir().unwrap();
        let base_path = temp.path().join("p/Base.java");
        let child_path = temp.path().join("p/Child.java");
        std::fs::create_dir_all(base_path.parent().unwrap()).unwrap();
        let base = "package p; abstract class Base {}";
        let child = "package p; class Child extends Base {}";
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index.index_file_with_source(&base_path, base).unwrap();
        index.index_file_with_source(&child_path, child).unwrap();
        let provider =
            TypeHierarchyProvider::new(Arc::clone(&index), Arc::new(SourceProviderChain::new()));
        let base_item = provider
            .create_type_hierarchy_item("p.Base")
            .unwrap()
            .unwrap();
        let child_item = provider
            .create_type_hierarchy_item("p.Child")
            .unwrap()
            .unwrap();
        assert!(provider.supertypes(&child_item).unwrap().is_some());
        assert!(provider.subtypes(&base_item).unwrap().is_some());
        assert!(provider.implementations(&base_item).unwrap().is_some());

        index.index_file_with_source(&child_path, child).unwrap();

        assert!(provider.supertypes(&child_item).unwrap().is_none());
        assert!(provider.subtypes(&base_item).unwrap().is_none());
        assert!(provider.implementations(&base_item).unwrap().is_none());
    }

    #[test]
    fn same_text_adoption_retires_old_item_and_immediately_prepares_a_new_one() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("p/Target.java");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = "package p; class Target {}";
        std::fs::write(&path, source).unwrap();
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index.index_file_sync_for_test(&path).unwrap();
        let file_id = index.get_file_id(&path).unwrap();
        let provider =
            TypeHierarchyProvider::new(Arc::clone(&index), Arc::new(SourceProviderChain::new()));
        let old_item = provider
            .prepare_type_hierarchy(file_id, Position::new(0, 17), source)
            .unwrap()
            .unwrap()
            .remove(0);

        assert!(index.adopt_caller_owned_revision_with_locks_held(&path, source, file_id));
        assert!(provider.supertypes(&old_item).unwrap().is_none());

        let new_item = provider
            .prepare_type_hierarchy(file_id, Position::new(0, 17), source)
            .unwrap()
            .expect("adopted current bytes retain a typed declaration proof")
            .remove(0);
        assert!(
            HierarchyItemProvenance::decode_current(&index, new_item.data.as_ref().unwrap())
                .is_some()
        );
    }

    #[test]
    fn java_prepare_fails_closed_when_request_bytes_are_not_indexed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("p/Target.java");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let indexed = "package p; class Target {}";
        let stale_request = "// not published\npackage p; class Target {}";
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index.index_file_with_source(&path, indexed).unwrap();
        let file_id = index.get_file_id(&path).unwrap();
        let provider = TypeHierarchyProvider::new(index, Arc::new(SourceProviderChain::new()));

        assert!(provider
            .prepare_type_hierarchy(file_id, Position::new(1, 17), stale_request)
            .unwrap()
            .is_none());
    }

    #[test]
    fn external_hierarchy_proof_binds_source_identity_and_graph_generation() {
        let index = WorkspaceIndex::new(PathBuf::from("/external-hierarchy-proof"));
        let symbol = type_symbol("dep.External", None);
        index.add_external_symbol(symbol.clone());
        let (provenance, definition) =
            HierarchyItemProvenance::capture(&index, "dep.External").unwrap();
        let data = provenance.encode();
        let expected_file_id = definition.location.file_id.as_u32().to_string();
        assert_eq!(
            data.get("e").and_then(serde_json::Value::as_str),
            Some(expected_file_id.as_str())
        );
        assert!(
            data.get("x").is_none(),
            "external paths are never serialized"
        );
        assert!(HierarchyItemProvenance::decode_current(&index, &data).is_some());

        let mut tampered = data.clone();
        tampered["e"] = serde_json::Value::String("0".to_string());
        assert!(HierarchyItemProvenance::decode_current(&index, &tampered).is_none());

        let mut legacy_path = data.clone();
        legacy_path.as_object_mut().unwrap().remove("e");
        legacy_path["x"] = serde_json::Value::String("jdk".to_string());
        assert!(HierarchyItemProvenance::decode_current(&index, &legacy_path).is_none());

        index.add_external_symbol(symbol);
        assert!(
            HierarchyItemProvenance::decode_current(&index, &data).is_none(),
            "same-source external replacement must retire the old graph proof"
        );
    }

    #[test]
    fn external_type_item_uses_attached_source_and_keeps_workspace_implementations() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir_all(root.join("app")).expect("workspace directory");
        let child_path = root.join("app/Child.java");
        let child_source = "package app; import dep.Base; public final class Child extends Base {}";
        std::fs::write(&child_path, child_source).expect("child source");

        let index = Arc::new(WorkspaceIndex::new(root));
        let root_symbol = Symbol {
            fqn: "dep.Root".into(),
            simple_name: "Root".into(),
            range: TextRange::new(0, 0),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers {
                is_public: true,
                is_abstract: true,
                ..Modifiers::default()
            },
            container: None,
            return_type: None,
            parameters: Vec::new(),
            parent_class: None,
            interfaces: Vec::new(),
            documentation: None,
            type_parameters: Vec::new(),
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        };
        let base_symbol = Symbol {
            fqn: "dep.Base".into(),
            simple_name: "Base".into(),
            range: TextRange::new(0, 0),
            kind: SymbolKind::CLASS,
            modifiers: Modifiers {
                is_public: true,
                is_abstract: true,
                ..Modifiers::default()
            },
            container: None,
            return_type: None,
            parameters: Vec::new(),
            parent_class: Some("dep.Root".into()),
            interfaces: Vec::new(),
            documentation: None,
            type_parameters: Vec::new(),
            is_deprecated: false,
            receiver_type: None,
            kotlin: None,
            throws: None,
        };

        let dependency_source = "package dep; public abstract class Base extends Root {}";
        let source_path: PathBuf = temp.path().join("attached/dep/Base.java");
        std::fs::create_dir_all(source_path.parent().expect("source parent"))
            .expect("source directory");
        std::fs::write(&source_path, dependency_source).expect("attached source");
        let root_source = "package dep; public abstract class Root {}";
        let root_source_path: PathBuf = temp.path().join("attached/dep/Root.java");
        std::fs::write(&root_source_path, root_source).expect("root attached source");
        let source_providers = Arc::new(SourceProviderChain::new());
        let source_id = PathBuf::from(crate::source_provider::jdk::JDK_SOURCE_ID);
        let source_identity = hyperion_workspace::ExternalSourceIdentity::for_virtual(
            crate::source_provider::jdk::JDK_SOURCE_ID,
        );
        let provider: Arc<dyn SourceProvider> = Arc::new(StaticSourceProvider {
            contents: [
                (
                    Arc::from("dep.Base"),
                    SourceContent::Stub {
                        source: Arc::new(SourceFile::new(Arc::from(dependency_source))),
                        temp_file: source_path.clone(),
                    },
                ),
                (
                    Arc::from("dep.Root"),
                    SourceContent::Stub {
                        source: Arc::new(SourceFile::new(Arc::from(root_source))),
                        temp_file: root_source_path,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        });
        let ticket = index.begin_external_source_refresh("type-hierarchy-test");
        assert!(source_providers.observe_group_refresh(&ticket));
        let provider_publication = source_providers
            .begin_group_publication(
                &ticket,
                std::slice::from_ref(&source_identity),
                Some(provider),
            )
            .expect("provider publication");
        let refresh = index
            .replace_external_sources_for_refresh(&ticket, std::slice::from_ref(&source_id))
            .expect("workspace publication");
        let revision = refresh
            .revision_for_identity(&source_identity)
            .expect("external source revision");
        assert!(index.add_external_symbols_responsive_for_revision(
            vec![root_symbol, base_symbol],
            revision,
        ));
        assert!(
            source_providers.record_group_publication_success_for_revision(
                &provider_publication,
                &source_identity,
                revision,
            )
        );
        assert!(source_providers.finish_group_publication(&provider_publication));
        index
            .index_file_with_source(&child_path, child_source)
            .expect("index child");

        let provider = TypeHierarchyProvider::new(index, source_providers);
        let base = provider
            .create_type_hierarchy_item("dep.Base")
            .expect("create item")
            .expect("attached dependency source is navigable");
        assert_eq!(base.uri, Url::from_file_path(source_path).unwrap());
        assert_ne!(base.selection_range, Range::default());

        let supertypes = provider
            .supertypes(&base)
            .expect("supertype query")
            .expect("external parent");
        assert_eq!(supertypes.len(), 1);
        assert_eq!(supertypes[0].detail.as_deref(), Some("dep.Root"));

        let implementations = provider
            .implementations(&base)
            .expect("implementation query")
            .expect("workspace subclass");
        assert_eq!(implementations.len(), 1);
        assert_eq!(implementations[0].detail.as_deref(), Some("app.Child"));
    }
}
