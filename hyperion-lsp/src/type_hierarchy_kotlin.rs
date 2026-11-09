// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin type hierarchy (`typeHierarchy/*`).
//!
//! Prepares on a `class`, `interface` or `object` and walks inheritance in both
//! directions, including edges that cross into Java.
//!
//! # Supertypes come off the CST, not the index
//!
//! `hyperion_kotlin::SymbolExtractor` qualifies an unqualified supertype with
//! the *declaring* file's package and nothing else, so `class Dog : Animal`
//! records `com.pets.Animal` even when `Animal` was imported from
//! `com.zoo`. Worse, `extract_object_decl` records no supertypes at all.
//! Reading the supertype list off the declaration and resolving each name
//! through that file's imports fixes both, and is what makes a Kotlin class
//! extending a *Java* class resolve. The indexed `parent_class` / `interfaces`
//! are still folded in, as a floor rather than the source of truth.
//!
//! # Subtypes consume the structural index
//!
//! The workspace maintains one canonical direct-inheritance graph for classes,
//! interfaces, records, enums, and objects. Type hierarchy reads its direct
//! edges; goto-implementation separately walks the graph to concrete leaves.
//!
//! Graph edges are keyed on the FQN the *extractor* computed, so they can miss
//! exactly the case the paragraph above describes. The gap is closed with a
//! reverse lookup over the usage index: a supertype clause emits a `TYPE_REF`
//! usage whose FQN *was* resolved through imports, so "every usage of this type
//! that sits in a supertype position" recovers the subtypes the structural
//! indices could not see — in either language.

use hyperion_kotlin::{SyntaxKind as K, SyntaxNode, SyntaxToken};
use hyperion_semantics::db::FileId;
use hyperion_workspace::{DeclarationNamespace, ImportMap, Language, SymbolTarget, WorkspaceIndex};
use rowan::{TextRange, TextSize};
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use tower_lsp::lsp_types::{Location, Position, Range, SymbolKind, TypeHierarchyItem, Url};
use tracing::debug;

use crate::call_hierarchy_kotlin::package_of;
use crate::kotlin_calls::is_name_token;
use crate::navigation_util::{offset_to_position, position_to_offset};
use crate::source_provider::SourceProviderChain;
use crate::type_hierarchy::{HierarchyItemProvenance, HierarchyRequestProvenance};

/// Type hierarchy for Kotlin buffers.
pub struct KotlinTypeHierarchyProvider {
    workspace_index: Arc<WorkspaceIndex>,
    source_providers: Arc<SourceProviderChain>,
}

impl KotlinTypeHierarchyProvider {
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
            debug!("Kotlin type hierarchy: workspace bindings changed during request");
            return Ok(None);
        }
        Ok(result)
    }

    // -----------------------------------------------------------------------
    // prepare
    // -----------------------------------------------------------------------

    /// The type under the cursor: the declaration it names, or the declaration
    /// a type reference points at.
    pub fn prepare(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        let Some(request) =
            HierarchyRequestProvenance::capture(&self.workspace_index, file_id, source_text)
        else {
            debug!("Kotlin type hierarchy: request bytes have no exact indexed revision");
            return Ok(None);
        };
        let result = self.prepare_at_revision(file_id, position, source_text)?;
        if !request.is_current(&self.workspace_index) {
            debug!("Kotlin type hierarchy: request source or graph changed during prepare");
            return Ok(None);
        }
        Ok(result)
    }

    fn prepare_at_revision(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        let offset = position_to_offset(source_text, position)
            .map_err(|e| format!("Failed to convert position: {e}"))?;

        // Parse the exact request bytes rather than a shared tree that may
        // already belong to a different document publication.
        let root = parse_kotlin(source_text);

        let Some(token) = token_at(&root, offset) else {
            return Ok(None);
        };
        // A cursor on `class`, `:` or a brace is not naming a type.
        if !is_name_token(token.kind()) {
            return Ok(None);
        }

        // A name inside a type reference (`: Animal`, `val x: Animal`) means the
        // type it refers to, not the declaration it happens to sit in.
        if in_type_reference(&token) {
            let package = package_of(&root);
            let imports = self.workspace_index.get_imports(file_id);
            let name = type_reference_name(&token).unwrap_or_else(|| token.text().to_string());
            let owners = declaration_at(&token)
                .map(|declaration| type_scope_chain(&declaration, &token))
                .unwrap_or_default();
            let resolved =
                self.resolve_type_name(&name, package.as_deref(), imports.as_ref(), &owners);
            return Ok(resolved
                .and_then(|fqn| self.item_for_fqn(&fqn))
                .map(|item| vec![item]));
        }

        let Some(declaration) = declaration_at(&token) else {
            return Ok(None);
        };
        let Some(name_token) = type_name_token(&declaration) else {
            return Ok(None);
        };
        if name_token.text_range() != token.text_range() {
            return Ok(None);
        }
        let package = package_of(&root);
        let name = name_token.text().to_string();
        let fqn = join_fqn(
            package.as_deref(),
            &enclosing_type_chain(&declaration),
            &name,
        );
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
        let Some(item) = declaration_item(
            &declaration,
            package.as_deref(),
            source_text,
            self.uri_for(file_id)?,
            &provenance,
        ) else {
            return Ok(None);
        };

        Ok(Some(vec![item]))
    }

    // -----------------------------------------------------------------------
    // supertypes
    // -----------------------------------------------------------------------

    /// Declared supertypes of `item`, resolved through the declaring file's
    /// imports and the workspace index.
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
        let Some(fqn) = self.target_fqn(item) else {
            debug!("Kotlin type hierarchy: cannot identify {}", item.name);
            return Ok(None);
        };

        let mut supertypes: Vec<TypeHierarchyItem> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();

        if let Some(declared) = self.declared_supertypes(&fqn) {
            // The current source spelling plus its import context is
            // authoritative. Appending the extractor's package-guessed edge
            // can create a second, false parent when that guessed FQN happens
            // to exist.
            for supertype in declared {
                if seen.insert(supertype.clone()) {
                    if let Some(item) = self.item_for_fqn(&supertype) {
                        supertypes.push(item);
                    }
                }
            }
        } else {
            // Cached/external declarations may have no readable Kotlin source.
            // Only then is the structural index the best available floor.
            if let Some(symbol) = self.workspace_index.get_symbol(&fqn) {
                let indexed = symbol
                    .parent_class
                    .iter()
                    .chain(symbol.interfaces.iter())
                    .map(|name| name.to_string());
                for supertype in indexed {
                    if seen.insert(supertype.clone()) {
                        if let Some(item) = self.item_for_fqn(&supertype) {
                            supertypes.push(item);
                        }
                    }
                }
            }
        }

        debug!(
            "Kotlin type hierarchy: {} supertype(s) of {fqn}",
            supertypes.len()
        );
        Ok((!supertypes.is_empty()).then_some(supertypes))
    }

    /// Supertype FQNs written on the declaration of `fqn`.
    fn declared_supertypes(&self, fqn: &str) -> Option<Vec<String>> {
        let location = self.workspace_index.find_symbol(fqn)?;
        let revisioned = self.workspace_index.revisioned_location(&location)?;
        let indexed = self
            .workspace_index
            .get_indexed_revision_content(revisioned.revision)?;
        if self.language_of(location.file_id) != Some(Language::Kotlin) {
            return None;
        }

        let declaration_offset = location.range.start;

        let root = parse_kotlin(&indexed.content);
        let package = package_of(&root);
        let imports = self.workspace_index.get_imports(location.file_id);

        // The declaration is the one whose name token sits at the indexed range.
        let declaration =
            token_at(&root, declaration_offset).and_then(|token| declaration_at(&token))?;

        Some(
            supertype_names(&declaration)
                .into_iter()
                .filter_map(|name| {
                    self.resolve_type_name(
                        &name,
                        package.as_deref(),
                        imports.as_ref(),
                        &enclosing_type_chain(&declaration),
                    )
                })
                .collect(),
        )
    }

    // -----------------------------------------------------------------------
    // subtypes
    // -----------------------------------------------------------------------

    /// Types that extend or implement `item`, in either language.
    pub fn subtypes(
        &self,
        item: &TypeHierarchyItem,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        self.with_stable_workspace(|| self.subtypes_at_revision(item))
    }

    fn subtypes_at_revision(
        &self,
        item: &TypeHierarchyItem,
    ) -> Result<Option<Vec<TypeHierarchyItem>>, String> {
        let Some(fqn) = self.target_fqn(item) else {
            debug!("Kotlin type hierarchy: cannot identify {}", item.name);
            return Ok(None);
        };

        // The usage-derived source overlaps the canonical graph and its
        // iteration order is unspecified, so dedupe and sort in one step.
        let subtypes = self.direct_subtype_fqns(&fqn);

        let items: Vec<TypeHierarchyItem> = subtypes
            .iter()
            .filter_map(|subtype| self.item_for_fqn(subtype))
            .collect();

        debug!("Kotlin type hierarchy: {} subtype(s) of {fqn}", items.len());
        Ok((!items.is_empty()).then_some(items))
    }

    /// Concrete transitive implementations of the type at `position`.
    ///
    /// Unlike `subtypes`, this is the goto-implementation semantic: abstract
    /// classes and subinterfaces are traversal nodes, not results.
    pub fn implementation_locations(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Vec<Location>>, String> {
        self.with_stable_workspace(|| {
            self.implementation_locations_at_revision(file_id, position, source_text)
        })
    }

    fn implementation_locations_at_revision(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
    ) -> Result<Option<Vec<Location>>, String> {
        let Some(items) = self.prepare(file_id, position, source_text)? else {
            return Ok(None);
        };
        let Some(target) = items.first() else {
            return Ok(None);
        };
        let Some(fqn) = self.target_fqn(target) else {
            return Ok(None);
        };

        let mut concrete = BTreeSet::new();
        let mut visited = BTreeSet::new();
        let mut pending = VecDeque::from([fqn.clone()]);
        while let Some(parent) = pending.pop_front() {
            if !visited.insert(parent.clone()) {
                continue;
            }
            for subtype in self.direct_subtype_fqns(&parent) {
                if !visited.contains(&subtype) {
                    pending.push_back(subtype.clone());
                }
                if self
                    .workspace_index
                    .get_symbol(&subtype)
                    .is_some_and(|symbol| self.is_concrete_implementation(&symbol))
                {
                    concrete.insert(subtype);
                }
            }
        }

        let locations: Vec<_> = concrete
            .iter()
            .filter_map(|fqn| self.item_for_fqn(fqn))
            .map(|item| Location {
                uri: item.uri,
                range: item.selection_range,
            })
            .collect();
        // `Some(empty)` is distinct from `None`: the cursor named a type, but
        // that type has no concrete implementation. The server uses `None` to
        // recognize non-type Kotlin cursors and safely fall back to ordinary
        // definition navigation (for example, a call expression).
        Ok(Some(locations))
    }

    fn is_concrete_implementation(&self, symbol: &hyperion_workspace::Symbol) -> bool {
        if symbol.kind == SymbolKind::INTERFACE || symbol.modifiers.is_abstract {
            return false;
        }
        if !symbol.modifiers.is_sealed {
            return true;
        }
        // Kotlin sealed classes are implicitly abstract. Older cached symbols
        // may predate that extracted modifier normalization; do not report
        // them as instantiable. Java sealed classes can be concrete.
        self.workspace_index
            .find_symbol(&symbol.fqn)
            .and_then(|location| self.language_of(location.file_id))
            != Some(Language::Kotlin)
    }

    fn direct_subtype_fqns(&self, fqn: &str) -> BTreeSet<String> {
        let mut subtypes = BTreeSet::new();
        subtypes.extend(
            self.workspace_index
                .find_direct_subtypes(fqn)
                .iter()
                .filter(|symbol| {
                    let Some(location) = self.workspace_index.find_symbol(&symbol.fqn) else {
                        return false;
                    };
                    if self.language_of(location.file_id) != Some(Language::Kotlin) {
                        return true;
                    }
                    // Kotlin's extractor historically package-qualified simple
                    // supertypes before imports were available. When readable
                    // source exists, admit the structural edge only if the CST
                    // plus current import context confirms it.
                    self.declared_supertypes(&symbol.fqn)
                        .is_none_or(|declared| declared.iter().any(|parent| parent == fqn))
                })
                .map(|symbol| symbol.fqn.to_string()),
        );
        subtypes.extend(self.subtypes_from_usages(fqn));
        subtypes.remove(fqn);
        subtypes
    }

    /// Reverse lookup: exact-current references to `fqn` that sit in a
    /// supertype list.
    ///
    /// The usage query re-resolves every candidate from its published bytes and
    /// retains both the workspace revision that selected the FQN and every
    /// scanned file revision (including files with no match). Keep that proof
    /// alive through CST inspection, then validate it once more before
    /// returning any semantic edge.
    fn subtypes_from_usages(&self, fqn: &str) -> Vec<String> {
        let usages = match self.workspace_index.find_current_usages(fqn) {
            Ok(usages) => usages,
            Err(error) => {
                debug!(
                    "Kotlin type hierarchy: exact usage lookup for {fqn} is unavailable: {error}"
                );
                return Vec::new();
            }
        };

        let mut found = BTreeSet::new();
        // CurrentUsageLocations is already grouped by file, so this remains
        // one parse per referencing file rather than one parse per reference.
        for file in &usages.files {
            if self.language_of(file.revision.file_id()) != Some(Language::Kotlin) {
                continue;
            }
            let Some(indexed) = self
                .workspace_index
                .get_indexed_revision_content(file.revision)
            else {
                // Do not return a partial hierarchy when one of the positive
                // inputs can no longer be reproduced exactly.
                return Vec::new();
            };
            let root = parse_kotlin(&indexed.content);
            let package = package_of(&root);

            for usage in &file.locations {
                let Some(token) = token_at(&root, usage.range.start) else {
                    continue;
                };
                let Some(declaration) = declaration_at(&token) else {
                    continue;
                };
                if !supertype_refs(&declaration)
                    .iter()
                    .any(|reference| reference.text_range().contains_range(token.text_range()))
                {
                    continue;
                }
                if let Some(name) = type_name_token(&declaration) {
                    found.insert(join_fqn(
                        package.as_deref(),
                        &enclosing_type_chain(&declaration),
                        name.text(),
                    ));
                }
            }
        }

        if !self
            .workspace_index
            .current_usage_locations_are_current(&usages)
        {
            debug!("Kotlin type hierarchy: usage inputs changed while resolving subtypes of {fqn}");
            return Vec::new();
        }

        found.into_iter().collect()
    }

    // -----------------------------------------------------------------------
    // Shared helpers
    // -----------------------------------------------------------------------

    /// Decode the exact declaration and graph proof echoed by the client.
    /// Missing or stale data must not be rebound by name.
    fn target_fqn(&self, item: &TypeHierarchyItem) -> Option<String> {
        let provenance =
            HierarchyItemProvenance::decode_current(&self.workspace_index, item.data.as_ref()?)?;
        self.workspace_index
            .get_symbol_target(&SymbolTarget::new(
                provenance.fqn(),
                DeclarationNamespace::Type,
            ))
            .filter(|symbol| is_type(symbol.kind))
            .map(|symbol| symbol.fqn.to_string())
    }

    /// Resolve a supertype name as written to an indexed FQN.
    fn resolve_type_name(
        &self,
        name: &str,
        package: Option<&str>,
        imports: Option<&ImportMap>,
        enclosing_types: &[String],
    ) -> Option<String> {
        let erased = erase_type_arguments(name);
        let name = erased.trim().trim_end_matches('?').trim();
        if name.is_empty() {
            return None;
        }

        let (head, suffix) = name
            .split_once('.')
            .map_or((name, None), |(head, suffix)| (head, Some(suffix)));
        let append_suffix = |binding: &str| {
            suffix.map_or_else(
                || binding.to_string(),
                |suffix| format!("{binding}.{suffix}"),
            )
        };

        // Lexically enclosing member types shadow package and on-demand
        // imports. Once the first segment binds, a missing suffix is a failed
        // reference, not permission to reinterpret the first segment.
        for depth in (1..=enclosing_types.len()).rev() {
            let owner = enclosing_types[..depth].join(".");
            let binding = package.map_or_else(
                || format!("{owner}.{head}"),
                |package| format!("{package}.{owner}.{head}"),
            );
            if self.workspace_index.has_class(&binding) {
                let candidate = append_suffix(&binding);
                return self
                    .workspace_index
                    .has_class(&candidate)
                    .then_some(candidate);
            }
        }

        if let Some(imports) = imports {
            if let Some(imported_head) = imports.explicit.get(head) {
                let candidate = append_suffix(imported_head);
                return self
                    .workspace_index
                    .has_class(&candidate)
                    .then_some(candidate);
            }
        }

        // A declaration in the current package binds the first segment and
        // shadows star-imported names, even when a requested suffix is absent.
        if let Some(package) = package {
            let binding = format!("{package}.{head}");
            if self.workspace_index.has_class(&binding) {
                let candidate = append_suffix(&binding);
                return self
                    .workspace_index
                    .has_class(&candidate)
                    .then_some(candidate);
            }
        }

        if let Some(imports) = imports {
            let mut bindings = BTreeSet::new();
            for wildcard in &imports.wildcards {
                let binding = format!("{wildcard}.{head}");
                if self.workspace_index.has_class(&binding) {
                    bindings.insert(binding);
                }
            }
            match bindings.len() {
                0 => {}
                1 => {
                    let candidate = append_suffix(&bindings.into_iter().next()?);
                    return self
                        .workspace_index
                        .has_class(&candidate)
                        .then_some(candidate);
                }
                _ => return None,
            }
        }

        // An indexed source-qualified package path is exact only after
        // contextual imports have had a chance to resolve the first segment.
        // Otherwise an unrelated default-package `Outer.Base` can steal
        // `import a.*; Outer.Base` from `a.Outer.Base`.
        if name.contains('.') && self.workspace_index.has_class(name) {
            return Some(name.to_string());
        }

        let mut default_bindings = BTreeSet::new();
        for default_package in KOTLIN_DEFAULT_IMPORT_PACKAGES {
            let binding = format!("{default_package}.{head}");
            if self.workspace_index.has_class(&binding) {
                default_bindings.insert(binding);
            }
        }
        match default_bindings.len() {
            0 => None,
            1 => {
                let candidate = append_suffix(&default_bindings.into_iter().next()?);
                self.workspace_index
                    .has_class(&candidate)
                    .then_some(candidate)
            }
            _ => None,
        }
    }

    /// Build an item for an indexed type.
    fn item_for_fqn(&self, fqn: &str) -> Option<TypeHierarchyItem> {
        let (provenance, definition) =
            HierarchyItemProvenance::capture(&self.workspace_index, fqn)?;
        // Read presentation metadata after capturing the proof. The final
        // fence below rejects a concurrent replacement, preventing a mixed-
        // generation item.
        let symbol = &definition.symbol;
        if !is_type(symbol.kind) {
            return None;
        }
        let location = crate::navigation_util::symbol_location_to_lsp_location(
            &self.workspace_index,
            &definition.location,
            Some(&self.source_providers),
        )
        .ok()??;
        if !provenance.is_current(&self.workspace_index) {
            return None;
        }

        Some(TypeHierarchyItem {
            name: symbol.simple_name.to_string(),
            kind: symbol.kind,
            tags: None,
            detail: Some(fqn.to_string()),
            uri: location.uri,
            range: location.range,
            selection_range: location.range,
            data: Some(provenance.encode()),
        })
    }

    fn language_of(&self, file_id: hyperion_workspace::FileId) -> Option<Language> {
        Language::from_path(&self.workspace_index.get_file_path(file_id)?)
    }

    fn uri_for(&self, file_id: FileId) -> Result<Url, String> {
        let path = self
            .workspace_index
            .get_file_path(file_id)
            .ok_or_else(|| format!("No path for file {file_id:?}"))?;
        Url::from_file_path(&path).map_err(|()| format!("Bad file path: {}", path.display()))
    }
}

// ---------------------------------------------------------------------------
// CST readers
// ---------------------------------------------------------------------------

fn parse_kotlin(source: &str) -> SyntaxNode {
    let tokens = hyperion_kotlin::Lexer::new(source).collect();
    let (green, _errors) = hyperion_kotlin::Parser::parse(tokens);
    SyntaxNode::new_root(green)
}

/// Token at `offset`, preferring an identifier over adjacent punctuation.
fn token_at(root: &SyntaxNode, offset: u32) -> Option<SyntaxToken> {
    let clamped = TextSize::from(offset).min(root.text_range().end());
    let at_offset = root.token_at_offset(clamped);
    let right = at_offset.clone().right_biased();
    if right
        .as_ref()
        .is_some_and(|token| is_name_token(token.kind()))
    {
        return right;
    }
    at_offset
        .left_biased()
        .filter(|token| is_name_token(token.kind()))
        .or(right)
}

/// The type declaration a token belongs to.
fn declaration_at(token: &SyntaxToken) -> Option<SyntaxNode> {
    let mut node = token.parent();
    while let Some(current) = node {
        if matches!(
            current.kind(),
            K::CLASS_DECL | K::OBJECT_DECL | K::INTERFACE_DECL | K::COMPANION_OBJECT
        ) {
            return Some(current);
        }
        node = current.parent();
    }
    None
}

/// Is this token part of a `TYPE_REF` rather than a declaration's own name?
fn in_type_reference(token: &SyntaxToken) -> bool {
    let mut node = token.parent();
    while let Some(current) = node {
        match current.kind() {
            K::TYPE_REF => return true,
            K::CLASS_DECL | K::OBJECT_DECL | K::INTERFACE_DECL | K::SOURCE_FILE => return false,
            _ => node = current.parent(),
        }
    }
    false
}

/// Complete source spelling of the nearest type reference. Using only the
/// token under the cursor loses the `Outer.` prefix from `Outer.Base`.
fn type_reference_name(token: &SyntaxToken) -> Option<String> {
    let mut node = token.parent();
    while let Some(current) = node {
        match current.kind() {
            K::TYPE_REF => return Some(current.text().to_string().trim().to_string()),
            K::CLASS_DECL | K::OBJECT_DECL | K::INTERFACE_DECL | K::SOURCE_FILE => return None,
            _ => node = current.parent(),
        }
    }
    None
}

/// Supertype names as written (`Animal`, `com.zoo.Speaker`, `List<String>`).
fn supertype_names(declaration: &SyntaxNode) -> Vec<String> {
    supertype_refs(declaration)
        .into_iter()
        .map(|node| node.text().to_string().trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// `TYPE_REF` children between the declaration's `:` and its body.
fn supertype_refs(declaration: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut refs = Vec::new();
    let mut seen_colon = false;

    for child in declaration.children_with_tokens() {
        match &child {
            rowan::NodeOrToken::Token(token) if token.kind() == K::COLON => seen_colon = true,
            rowan::NodeOrToken::Token(token) if token.kind() == K::L_BRACE => break,
            rowan::NodeOrToken::Node(node) if node.kind() == K::CLASS_BODY => break,
            rowan::NodeOrToken::Node(node) if seen_colon && node.kind() == K::TYPE_REF => {
                refs.push(node.clone());
            }
            _ => {}
        }
    }

    refs
}

/// Name token of a type declaration.
fn type_name_token(declaration: &SyntaxNode) -> Option<SyntaxToken> {
    declaration
        .children_with_tokens()
        .filter_map(|child| child.into_token())
        .find(|token| is_name_token(token.kind()))
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

/// Lexical type owners visible at `token`. A declaration's own nested types
/// are visible throughout its body, but not in its supertype header.
fn type_scope_chain(declaration: &SyntaxNode, token: &SyntaxToken) -> Vec<String> {
    let mut owners = enclosing_type_chain(declaration);
    let in_body = declaration
        .children()
        .find(|child| child.kind() == K::CLASS_BODY)
        .is_some_and(|body| body.text_range().contains_range(token.text_range()));
    if in_body {
        owners.push(
            type_name_token(declaration)
                .map_or_else(|| "Companion".to_string(), |name| name.text().to_string()),
        );
    }
    owners
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

/// Build the item for a declaration found in the buffer being edited.
fn declaration_item(
    declaration: &SyntaxNode,
    package: Option<&str>,
    source: &str,
    uri: Url,
    provenance: &HierarchyItemProvenance,
) -> Option<TypeHierarchyItem> {
    let name_token = type_name_token(declaration)?;
    let name = name_token.text().to_string();
    let fqn = join_fqn(package, &enclosing_type_chain(declaration), &name);

    let range = to_lsp_range(source, declaration.text_range());
    Some(TypeHierarchyItem {
        name,
        kind: kind_of(declaration),
        tags: None,
        detail: Some(fqn.clone()),
        uri,
        range,
        selection_range: to_lsp_range(source, name_token.text_range()),
        data: Some(provenance.encode()),
    })
}

fn kind_of(declaration: &SyntaxNode) -> SymbolKind {
    match declaration.kind() {
        K::INTERFACE_DECL => SymbolKind::INTERFACE,
        K::OBJECT_DECL | K::COMPANION_OBJECT => SymbolKind::OBJECT,
        _ => SymbolKind::CLASS,
    }
}

fn to_lsp_range(source: &str, range: TextRange) -> Range {
    Range {
        start: offset_to_position(source, usize::from(range.start())),
        end: offset_to_position(source, usize::from(range.end())),
    }
}

/// Kinds a type hierarchy can contain.
const fn is_type(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::CLASS
            | SymbolKind::INTERFACE
            | SymbolKind::OBJECT
            | SymbolKind::ENUM
            | SymbolKind::STRUCT
    )
}

const KOTLIN_DEFAULT_IMPORT_PACKAGES: &[&str] = &[
    "kotlin",
    "kotlin.annotation",
    "kotlin.collections",
    "kotlin.comparisons",
    "kotlin.io",
    "kotlin.ranges",
    "kotlin.sequences",
    "kotlin.text",
    "java.lang",
    "kotlin.jvm",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_text_republication_rejects_kotlin_hierarchy_items() {
        let temp = tempfile::tempdir().expect("tempdir");
        let base_path = temp.path().join("p/Base.kt");
        let child_path = temp.path().join("p/Child.kt");
        std::fs::create_dir_all(base_path.parent().unwrap()).expect("package directory");
        let base = "package p\nabstract class Base\n";
        let child = "package p\nclass Child : Base()\n";
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index.index_file_with_source(&base_path, base).unwrap();
        index.index_file_with_source(&child_path, child).unwrap();
        let provider = KotlinTypeHierarchyProvider::new(
            Arc::clone(&index),
            Arc::new(SourceProviderChain::new()),
        );
        let base_item = provider.item_for_fqn("p.Base").expect("base item");
        let child_item = provider.item_for_fqn("p.Child").expect("child item");
        assert!(provider.supertypes(&child_item).unwrap().is_some());
        assert!(provider.subtypes(&base_item).unwrap().is_some());

        index.index_file_with_source(&child_path, child).unwrap();

        assert!(provider.supertypes(&child_item).unwrap().is_none());
        assert!(provider.subtypes(&base_item).unwrap().is_none());
        assert!(provider.target_fqn(&child_item).is_none());
    }

    #[test]
    fn prepare_fails_closed_when_request_bytes_are_not_the_indexed_revision() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("p/Target.kt");
        std::fs::create_dir_all(path.parent().unwrap()).expect("package directory");
        let indexed = "package p\nclass Target\n";
        let stale_request = "// not published\npackage p\nclass Target\n";
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index.index_file_with_source(&path, indexed).unwrap();
        let file_id = index.get_file_id(&path).unwrap();
        let provider =
            KotlinTypeHierarchyProvider::new(index, Arc::new(SourceProviderChain::new()));

        assert!(provider
            .prepare(file_id, Position::new(2, 6), stale_request)
            .unwrap()
            .is_none());
    }

    #[test]
    fn usage_subtype_uses_ranges_from_the_republished_file_revision() {
        let temp = tempfile::tempdir().expect("tempdir");
        let base_path = temp.path().join("base/Base.kt");
        let child_path = temp.path().join("p/Child.kt");
        std::fs::create_dir_all(base_path.parent().unwrap()).expect("base parent");
        std::fs::create_dir_all(child_path.parent().unwrap()).expect("child parent");
        let base = "package base\ninterface Base\n";
        let child = "package p\nimport base.Base\nclass Child : Base\n";
        std::fs::write(&base_path, base).expect("base source");
        std::fs::write(&child_path, child).expect("child source");

        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(&base_path, base)
            .expect("index base");
        index
            .index_file_with_source(&child_path, child)
            .expect("index child");
        let child_id = index.get_file_id(&child_path).expect("child file ID");
        let initial_revision = index
            .current_file_revision(child_id)
            .expect("initial child revision");

        let shifted = format!("// shifts the recorded usage range\n{child}");
        index
            .index_file_with_source(&child_path, &shifted)
            .expect("publish shifted child");
        assert_ne!(
            index.current_file_revision(child_id),
            Some(initial_revision),
            "same-path publication must mint a fresh range identity"
        );

        let provider =
            KotlinTypeHierarchyProvider::new(index, Arc::new(SourceProviderChain::new()));
        assert_eq!(
            provider.subtypes_from_usages("base.Base"),
            vec!["p.Child"],
            "the refreshed usage range still identifies Child's supertype clause"
        );
    }
}
