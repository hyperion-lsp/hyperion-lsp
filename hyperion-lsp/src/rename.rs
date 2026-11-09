// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Rename Provider for LSP
//!
//! This module implements rename functionality for the Hyperion LSP.
//! It provides the ability to rename symbols (variables, methods, classes, etc.)
//! across the workspace.

use hyperion_semantics::db::{
    Database, FileId, Language, ParseDatabase, SemanticDatabase, SourceDatabase,
    WorkspaceIndexHandle,
};
use hyperion_semantics::resolver::SymbolResolver;
use hyperion_workspace::{
    CurrentUsageQueryError, DeclarationNamespace, FileRevisionToken, QueryEngine,
    RevisionedLocation, SymbolTarget, WorkspaceIndex, WorkspaceRevision,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tower_lsp::lsp_types::{
    Location, Position, Range, SymbolKind as LspSymbolKind, TextEdit, Url, WorkspaceEdit,
};
use tower_lsp::CancellationToken;
use tracing::debug;

use crate::navigation::NavigationProvider;
use crate::position_utils::LineIndex;
use crate::semantic_host::ReadTxn;

/// Provides rename functionality for symbols
#[derive(Clone)]
pub struct RenameProvider {
    workspace_index: Arc<WorkspaceIndex>,
    query_engine: Arc<QueryEngine>,
    source_providers: Arc<crate::source_provider::SourceProviderChain>,
}

#[derive(Debug)]
pub(crate) struct RenameProof {
    pub(crate) edit: WorkspaceEdit,
    workspace_revision: Option<WorkspaceRevision>,
    scanned_revisions: Vec<FileRevisionToken>,
    resource_proof: Option<Arc<hyperion_workspace::RefactoringResourceProof>>,
    pub(crate) file_rename: Option<FileRenamePlan>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRenamePlan {
    pub old_uri: Url,
    pub new_uri: Url,
}

/// Semantic rename result before client capability/version fencing.
///
/// A public top-level Java type can require a source-file move in addition to
/// text edits. Keeping that operation explicit prevents non-LSP callers from
/// accidentally applying only the textual half of the refactoring.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameResult {
    pub edit: WorkspaceEdit,
    pub file_rename: Option<FileRenamePlan>,
}

impl RenameProof {
    pub(crate) fn is_current_cancellable<F>(
        &self,
        workspace_index: &WorkspaceIndex,
        is_cancelled: &F,
    ) -> bool
    where
        F: Fn() -> bool + ?Sized,
    {
        let sources_are_current = || {
            !is_cancelled()
                && self.workspace_revision.map_or_else(
                    || {
                        self.scanned_revisions.iter().all(|revision| {
                            workspace_index.current_file_revision(revision.file_id())
                                == Some(*revision)
                        })
                    },
                    |workspace_revision| {
                        workspace_index.workspace_revisions_are_current(
                            workspace_revision,
                            &self.scanned_revisions,
                        )
                    },
                )
        };
        sources_are_current()
            && self
                .resource_proof
                .as_ref()
                .is_none_or(|proof| proof.is_current_cancellable(is_cancelled))
            && !is_cancelled()
            && self
                .file_rename
                .as_ref()
                .is_none_or(file_rename_plan_is_current)
            && sources_are_current()
    }

    pub(crate) const fn requires_versioned_workspace_edit(&self) -> bool {
        self.workspace_revision.is_some()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RenameToken {
    pub name: String,
    pub range: Range,
}

#[derive(Clone, Debug)]
struct RenameTarget {
    token: RenameToken,
    definition: Location,
    definition_file_id: FileId,
    definition_revision: FileRevisionToken,
    workspace_revision: WorkspaceRevision,
    fqn: Option<String>,
    kind: Option<LspSymbolKind>,
    language: Language,
}

#[derive(Clone, Debug)]
struct RevisionedRenameLocation {
    revision: FileRevisionToken,
    location: Location,
    /// Text that must currently be at `location` and its replacement, when they
    /// differ from the rename's own old/new name: a Java `getNote()` call site
    /// of the renamed Kotlin property `note` becomes `getRemark()`.
    spelling: Option<(String, String)>,
}

#[derive(Default)]
struct RevisionedRenameReferences {
    locations: Vec<RevisionedRenameLocation>,
    guards: std::collections::HashSet<FileRevisionToken>,
    workspace_revision: Option<WorkspaceRevision>,
    resource_proof: Option<Arc<hyperion_workspace::RefactoringResourceProof>>,
}

impl RevisionedRenameReferences {
    fn insert_guard(&mut self, revision: FileRevisionToken) {
        self.guards.insert(revision);
    }

    fn push(&mut self, revision: FileRevisionToken, location: Location) {
        self.push_spelled(revision, location, None);
    }

    fn push_spelled(
        &mut self,
        revision: FileRevisionToken,
        location: Location,
        spelling: Option<(String, String)>,
    ) {
        self.insert_guard(revision);
        if !self
            .locations
            .iter()
            .any(|existing| existing.revision == revision && existing.location == location)
        {
            self.locations.push(RevisionedRenameLocation {
                revision,
                location,
                spelling,
            });
        }
    }
}

impl RenameProvider {
    /// Creates a new RenameProvider
    pub const fn new(
        workspace_index: Arc<WorkspaceIndex>,
        query_engine: Arc<QueryEngine>,
        source_providers: Arc<crate::source_provider::SourceProviderChain>,
    ) -> Self {
        Self {
            workspace_index,
            query_engine,
            source_providers,
        }
    }

    /// Check if a symbol at the given position can be renamed
    ///
    /// Performs both syntactic and semantic validation to ensure the rename is safe:
    /// - Checks the language-specific token is an identifier, not a keyword or literal
    /// - Resolves the identifier to its declaration
    /// - Requires the declaration to live in a non-external workspace file
    ///
    /// Returns the range of the symbol if it can be renamed, None otherwise
    pub fn prepare_rename(
        &self,
        file_id: FileId,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<Range>, String> {
        debug!("Preparing rename at {:?}", position);

        let request_revision = self.require_request_revision(file_id, source_text, uri)?;
        Ok(self
            .resolve_target(file_id, request_revision, position, source_text, uri)?
            .map(|target| target.token.range))
    }

    /// Perform the rename operation
    ///
    /// Returns a WorkspaceEdit containing all the text edits needed for the rename
    pub fn rename(
        &self,
        file_id: FileId,
        position: Position,
        new_name: &str,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<RenameResult>, String> {
        self.rename_cancellable(
            file_id,
            position,
            new_name,
            source_text,
            uri,
            &CancellationToken::default(),
        )
    }

    /// Perform a rename while cooperatively observing LSP request cancellation.
    pub fn rename_cancellable(
        &self,
        file_id: FileId,
        position: Position,
        new_name: &str,
        source_text: &str,
        uri: &Url,
        cancellation: &CancellationToken,
    ) -> Result<Option<RenameResult>, String> {
        self.rename_cancellable_with_workspace_completeness(
            file_id,
            position,
            new_name,
            source_text,
            uri,
            true,
            cancellation,
        )
    }

    /// Production rename entry point. Local bindings need only their exact
    /// file revision; any indexed declaration requires an authoritative source
    /// manifest and complete dependency model.
    #[allow(clippy::too_many_arguments)]
    pub fn rename_cancellable_with_workspace_completeness(
        &self,
        file_id: FileId,
        position: Position,
        new_name: &str,
        source_text: &str,
        uri: &Url,
        workspace_complete: bool,
        cancellation: &CancellationToken,
    ) -> Result<Option<RenameResult>, String> {
        self.rename_proof_cancellable_with_workspace_completeness(
            file_id,
            position,
            new_name,
            source_text,
            uri,
            workspace_complete,
            cancellation,
        )
        .map(|proof| {
            proof.map(|proof| RenameResult {
                edit: proof.edit,
                file_rename: proof.file_rename,
            })
        })
    }

    /// Production rename proof retained through the protocol application
    /// fence. Negative source and resource scans are as important as positive
    /// edit locations: either can gain a reference while a request is running.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rename_proof_cancellable_with_workspace_completeness(
        &self,
        file_id: FileId,
        position: Position,
        new_name: &str,
        source_text: &str,
        uri: &Url,
        workspace_complete: bool,
        cancellation: &CancellationToken,
    ) -> Result<Option<RenameProof>, String> {
        if cancellation.is_cancelled() {
            return Ok(None);
        }

        let request_revision = self.require_request_revision(file_id, source_text, uri)?;

        debug!("Renaming symbol at {:?} to '{}'", position, new_name);

        let Some(target) =
            self.resolve_target(file_id, request_revision, position, source_text, uri)?
        else {
            return Ok(None);
        };

        if target
            .fqn
            .as_deref()
            .is_some_and(|fqn| self.workspace_index.jvm_origin_of(fqn).is_some())
        {
            return Err(
                "Cannot rename a Java-visible member of a Kotlin declaration (accessor, file \
                 facade function or static forwarder); rename the Kotlin declaration instead"
                    .to_string(),
            );
        }

        if target.fqn.is_some() && !workspace_complete {
            return Err(
                "Cannot rename atomically: workspace source and dependency indexing is incomplete"
                    .to_string(),
            );
        }

        if !is_valid_identifier(target.language, target.kind, new_name) {
            return Err(format!(
                "'{new_name}' is not a valid {} identifier",
                target.language.name()
            ));
        }
        if !matches!(target.language, Language::Java | Language::Kotlin) {
            return Err(format!(
                "Cannot rename atomically: {} reference scanning is not implemented",
                target.language.name()
            ));
        }
        if target.kind != Some(LspSymbolKind::METHOD) {
            self.reject_conflict(&target, new_name)?;
        }
        let file_rename = self.top_level_java_file_rename(&target, new_name)?;

        let mut references = self.collect_references(&target, new_name, cancellation)?;

        if target.fqn.is_some() && references.resource_proof.is_none() {
            references.resource_proof = Some(
                self.workspace_index
                    .capture_refactoring_resource_proof(
                        [Arc::<str>::from(target.token.name.as_str())],
                        &|| cancellation.is_cancelled(),
                    )
                    .map_err(|error| {
                        format!(
                            "Cannot rename atomically: resource usage proof is unavailable: {error}"
                        )
                    })?,
            );
        }

        if cancellation.is_cancelled() {
            return Ok(None);
        }

        if references.locations.is_empty() {
            debug!("No references found for symbol at position");
            return Ok(None);
        }

        self.validate_new_name_for_edit_languages(&references, target.kind, new_name)?;

        debug!("Found {} references for rename", references.locations.len());
        let Some(workspace_edit) =
            self.assemble_workspace_edit(&references, &target.token.name, new_name, cancellation)?
        else {
            return Ok(None);
        };

        let mut scanned_revisions = references.guards.iter().copied().collect::<Vec<_>>();
        scanned_revisions.sort_unstable_by_key(|revision| revision.file_id().as_u32());
        let proof = RenameProof {
            edit: workspace_edit,
            workspace_revision: references.workspace_revision,
            scanned_revisions,
            resource_proof: references.resource_proof,
            file_rename,
        };
        if !proof.is_current_cancellable(&self.workspace_index, &|| cancellation.is_cancelled()) {
            return Err(
                "Cannot rename atomically: a negative usage input changed; try again".to_string(),
            );
        }
        Ok(Some(proof))
    }

    fn top_level_java_file_rename(
        &self,
        target: &RenameTarget,
        new_name: &str,
    ) -> Result<Option<FileRenamePlan>, String> {
        if target.language != Language::Java
            || new_name == target.token.name
            || target.kind.is_none_or(|kind| {
                DeclarationNamespace::for_kind(kind) != DeclarationNamespace::Type
            })
        {
            return Ok(None);
        }
        let fqn = target.fqn.as_deref().ok_or_else(|| {
            "Cannot rename a top-level Java type without its indexed identity".to_string()
        })?;
        let symbol = self
            .workspace_index
            .get_symbol_target(&SymbolTarget::new(fqn, DeclarationNamespace::Type))
            .ok_or_else(|| format!("Cannot rename: indexed type '{fqn}' is unavailable"))?;
        if !symbol.modifiers.is_public || symbol.container.is_some() {
            return Ok(None);
        }

        let old_path = target.definition.uri.to_file_path().map_err(|()| {
            "Cannot rename a public top-level Java type declared at a non-file URI".to_string()
        })?;
        let indexed = self.indexed_content(target.definition_revision)?;
        if indexed.file_path != old_path {
            return Err(
                "Cannot rename: the declaration URI no longer matches its indexed path".to_string(),
            );
        }
        let is_java_file = old_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("java"));
        let stem_matches = old_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem == target.token.name);
        if !is_java_file || !stem_matches {
            return Ok(None);
        }

        let old_metadata = std::fs::symlink_metadata(&old_path).map_err(|error| {
            format!(
                "Cannot rename public type '{}': source file '{}' is unavailable: {error}",
                target.token.name,
                old_path.display()
            )
        })?;
        if !old_metadata.file_type().is_file() {
            return Err(format!(
                "Cannot rename public type '{}': '{}' is not a regular source file",
                target.token.name,
                old_path.display()
            ));
        }

        let new_path = old_path.with_file_name(format!("{new_name}.java"));
        if self
            .workspace_index
            .get_file_id(&new_path)
            .is_some_and(|file_id| file_id != target.definition_file_id)
            || file_rename_destination_collides(&old_path, &new_path).map_err(|error| {
                format!(
                    "Cannot inspect Java rename target '{}': {error}",
                    new_path.display()
                )
            })?
        {
            return Err(format!(
                "Cannot rename public type '{}' to '{}': target file '{}' already exists",
                target.token.name,
                new_name,
                new_path.display()
            ));
        }
        let new_uri = Url::from_file_path(&new_path).map_err(|()| {
            format!(
                "Cannot rename public type '{}': target path '{}' is not a file URI",
                target.token.name,
                new_path.display()
            )
        })?;
        Ok(Some(FileRenamePlan {
            old_uri: target.definition.uri.clone(),
            new_uri,
        }))
    }

    fn require_request_revision(
        &self,
        file_id: FileId,
        source_text: &str,
        uri: &Url,
    ) -> Result<FileRevisionToken, String> {
        let path = uri
            .to_file_path()
            .map_err(|()| "Cannot rename a non-workspace document".to_string())?;
        if self.workspace_index.get_file_id(&path) != Some(file_id) {
            return Err(
                "Cannot rename: the request document is not bound to this file revision"
                    .to_string(),
            );
        }
        let revision = self
            .workspace_index
            .current_file_revision(file_id)
            .ok_or_else(|| "Cannot rename: the request file has no indexed revision".to_string())?;
        let indexed = self.indexed_content(revision)?;
        if indexed.content.as_str() != source_text || indexed.file_path != path {
            return Err(
                "Cannot rename: the document changed before its index revision was published; try again"
                    .to_string(),
            );
        }
        Ok(revision)
    }

    fn indexed_content(
        &self,
        revision: FileRevisionToken,
    ) -> Result<hyperion_workspace::IndexedContent, String> {
        self.workspace_index
            .get_indexed_revision_content(revision)
            .ok_or_else(|| {
                format!(
                    "Cannot rename atomically: indexed revision {:?} was retired; try again",
                    revision
                )
            })
    }

    fn reject_conflict(&self, target: &RenameTarget, new_name: &str) -> Result<(), String> {
        if new_name == target.token.name {
            return Ok(());
        }

        let conflict = if let Some(fqn) = target.fqn.as_deref() {
            self.workspace_conflict(fqn, target.kind, new_name)?
        } else {
            match target.language {
                Language::Java => self.java_local_conflict(target, new_name)?,
                Language::Kotlin => self.kotlin_local_conflict(target, new_name)?,
                Language::Groovy | Language::Scala => false,
            }
        };

        if conflict {
            return Err(format!(
                "Cannot rename '{}' to '{}': '{}' already exists in the same scope",
                target.token.name, new_name, new_name
            ));
        }
        Ok(())
    }

    fn workspace_conflict(
        &self,
        target_fqn: &str,
        target_kind: Option<LspSymbolKind>,
        new_name: &str,
    ) -> Result<bool, String> {
        let target_kind = target_kind.ok_or_else(|| {
            format!("Cannot rename: declaration kind for '{target_fqn}' is unavailable")
        })?;
        let target_namespace = DeclarationNamespace::for_kind(target_kind);
        let target_symbol = self
            .workspace_index
            .get_symbol_target(&SymbolTarget::new(target_fqn, target_namespace))
            .ok_or_else(|| {
                format!("Cannot rename: indexed target '{target_fqn}' is no longer available")
            })?;
        let target_scope = symbol_scope_key(&target_symbol);

        for candidate in self.workspace_index.find_by_simple_name(new_name) {
            let candidate_namespace = DeclarationNamespace::for_kind(candidate.kind);
            if candidate.fqn.as_ref() == target_fqn || candidate_namespace != target_namespace {
                continue;
            }
            let candidate_symbol = self
                .workspace_index
                .get_symbol_target(&SymbolTarget::new(
                    candidate.fqn.clone(),
                    candidate_namespace,
                ))
                .ok_or_else(|| {
                    format!(
                        "Cannot rename: conflict candidate '{}' is no longer available",
                        candidate.fqn
                    )
                })?;
            if symbol_scope_key(&candidate_symbol) == target_scope {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn java_local_conflict(&self, target: &RenameTarget, new_name: &str) -> Result<bool, String> {
        let source = self.indexed_content(target.definition_revision)?;
        let definition_offset = position_to_offset(&source.content, target.definition.range.start)
            .map_err(|error| format!("Cannot rename: invalid declaration position: {error}"))?;
        let symbol_table =
            java_symbol_table_from_source(target.definition_file_id, &source.content);
        let mut resolver = SymbolResolver::with_cached_table(symbol_table);
        let scope = resolver
            .find_scope_at_offset(definition_offset)
            .ok_or_else(|| {
                "Cannot rename: declaration scope is unavailable in the indexed revision"
                    .to_string()
            })?;
        let symbol = resolver
            .resolve_in_scope(&target.token.name, scope)
            .ok_or_else(|| {
                "Cannot rename: declaration binding is unavailable in the indexed revision"
                    .to_string()
            })?;
        let owner_scope = resolver
            .symbol_table()
            .find_scope_for_symbol(symbol.symbol_id)
            .ok_or_else(|| "Cannot rename: declaration owner scope is unavailable".to_string())?;

        Ok(resolver
            .symbol_table()
            .get_scope(owner_scope)
            .and_then(|scope| scope.lookup_local(new_name))
            .is_some_and(|candidate| candidate != symbol.symbol_id))
    }

    fn kotlin_local_conflict(&self, target: &RenameTarget, new_name: &str) -> Result<bool, String> {
        let source = self.indexed_content(target.definition_revision)?;
        let definition_offset = position_to_offset(&source.content, target.definition.range.start)
            .map_err(|error| format!("Cannot rename: invalid declaration position: {error}"))?;
        let exact = ReadTxn::scratch(|db| {
            db.set_file_text_tracked(
                target.definition_file_id,
                Arc::new(source.content.as_str().to_string()),
            );
            db.set_file_language(target.definition_file_id, Language::Kotlin);
        });
        let scope_graph = exact.scope_graph(target.definition_file_id);

        let definition_range = lsp_range_to_text_range(&source.content, target.definition.range)?;

        Ok(scope_graph
            .resolve_symbol(new_name, definition_offset as usize)
            .is_some_and(|candidate| candidate.definition_range != definition_range))
    }

    fn collect_references(
        &self,
        target: &RenameTarget,
        new_name: &str,
        cancellation: &CancellationToken,
    ) -> Result<RevisionedRenameReferences, String> {
        let mut references = if target.kind == Some(LspSymbolKind::METHOD) {
            self.collect_callable_references(target, new_name, cancellation)?
        } else if target.fqn.is_some() {
            self.collect_indexed_symbol_references(target, new_name, cancellation)?
        } else {
            self.collect_local_references(target, cancellation)?
        };
        references.insert_guard(target.definition_revision);

        if !references
            .locations
            .iter()
            .any(|reference| reference.location == target.definition)
        {
            references.push(target.definition_revision, target.definition.clone());
        }

        references.locations.sort_by(|a, b| {
            a.location
                .uri
                .as_str()
                .cmp(b.location.uri.as_str())
                .then(
                    a.location
                        .range
                        .start
                        .line
                        .cmp(&b.location.range.start.line),
                )
                .then(
                    a.location
                        .range
                        .start
                        .character
                        .cmp(&b.location.range.start.character),
                )
        });
        references
            .locations
            .dedup_by(|a, b| a.revision == b.revision && a.location == b.location);
        Ok(references)
    }

    /// Indexed declarations use the workspace index's strict current-usage
    /// proof. Besides yielding positive edits, that proof audits every source
    /// revision and every configured resource for unresolved same-spelled
    /// references. Falling back to navigation over a textual scan here would
    /// turn extractor omissions into silently partial renames.
    fn collect_indexed_symbol_references(
        &self,
        target: &RenameTarget,
        new_name: &str,
        cancellation: &CancellationToken,
    ) -> Result<RevisionedRenameReferences, String> {
        let fqn = target.fqn.as_deref().ok_or_else(|| {
            "Cannot rename atomically: indexed declaration identity is unavailable".to_string()
        })?;
        let kind = target.kind.ok_or_else(|| {
            "Cannot rename atomically: indexed declaration namespace is unavailable".to_string()
        })?;
        let symbol_target = SymbolTarget::new(fqn, DeclarationNamespace::for_kind(kind));
        // A Kotlin declaration is also what Java callers see: `getNote()`,
        // `SquareKt.f()`. Their call sites move with it, spelled the JVM way.
        let family = self.workspace_index.jvm_reference_family(&symbol_target);
        let alias_spellings = self.jvm_alias_spellings(&family, &target.token.name, new_name);
        let usages = match self
            .workspace_index
            .find_current_usages_for_symbol_targets_for_refactoring(
                &family,
                target.workspace_revision,
                &[],
                || cancellation.is_cancelled(),
            ) {
            Ok(usages) => usages,
            Err(CurrentUsageQueryError::Cancelled) if cancellation.is_cancelled() => {
                return Ok(RevisionedRenameReferences::default());
            }
            Err(error) => {
                return Err(format!(
                    "Cannot rename atomically: symbol usage provenance is unavailable: {error}"
                ));
            }
        };

        let mut references = RevisionedRenameReferences {
            workspace_revision: Some(target.workspace_revision),
            resource_proof: usages.resource_proof().cloned(),
            ..RevisionedRenameReferences::default()
        };
        for &revision in usages.scanned_revisions() {
            references.insert_guard(revision);
        }
        for file in &usages.files {
            for location in &file.locations {
                let revisioned = RevisionedLocation {
                    revision: file.revision,
                    location: location.clone(),
                };
                let lsp_location =
                    crate::navigation_util::revisioned_symbol_location_to_lsp_location(
                        &self.workspace_index,
                        &revisioned,
                    )?
                    .ok_or_else(|| {
                        format!(
                            "Cannot rename atomically: usage '{}' has no current source range",
                            location.fqn
                        )
                    })?;
                // A call whose argument types were unresolved is recorded with
                // a wildcard descriptor (`setNote(_)`), so match on `owner.name`.
                let spelling = alias_spellings
                    .get(location.fqn.split('(').next().unwrap_or(&location.fqn))
                    .cloned();
                references.push_spelled(file.revision, lsp_location, spelling);
            }
        }
        if symbol_target.namespace == DeclarationNamespace::Type {
            // Constructor declarations spell the type's source name but are
            // declarations, not usage-cache entries. They must nevertheless
            // move with a type rename. Restrict them to the exact elected
            // declaration revision so a duplicate-FQN shadow source cannot
            // be spliced into the edit.
            for file in self
                .workspace_index
                .find_by_simple_name_revisioned(&target.token.name)
                .files
                .into_iter()
                .filter(|file| file.revision == target.definition_revision)
            {
                for constructor in file.locations.into_iter().filter(|location| {
                    location.kind == LspSymbolKind::CONSTRUCTOR
                        && location.container_name.as_deref() == Some(fqn)
                }) {
                    let revisioned = RevisionedLocation {
                        revision: file.revision,
                        location: constructor,
                    };
                    let location =
                        crate::navigation_util::revisioned_symbol_location_to_lsp_location(
                            &self.workspace_index,
                            &revisioned,
                        )?
                        .ok_or_else(|| {
                            "Cannot rename atomically: constructor declaration revision was retired"
                                .to_string()
                        })?;
                    references.push(file.revision, location);
                }
            }
        }
        if !self
            .workspace_index
            .current_usage_locations_are_current(&usages)
        {
            return Err(
                "Cannot rename atomically: symbol usages changed during the request; try again"
                    .to_string(),
            );
        }
        Ok(references)
    }

    /// Old and new spelling of every JVM-view alias in `family`, by `owner.name`
    /// (the FQN without its parameter descriptor).
    ///
    /// Kotlin `note` is read as `getNote()` and written as `setNote(..)`; a
    /// rename to `remark` must produce `getRemark()` / `setRemark(..)`. A
    /// function alias keeps its `@JvmName` (and is left alone) unless it is
    /// spelled like the Kotlin function.
    fn jvm_alias_spellings(
        &self,
        family: &[SymbolTarget],
        old_name: &str,
        new_name: &str,
    ) -> HashMap<String, (String, String)> {
        let (old_getter, old_setter) = hyperion_types::jvm_names::accessor_names(old_name);
        let (new_getter, new_setter) = hyperion_types::jvm_names::accessor_names(new_name);
        let mut spellings = HashMap::new();
        for member in family {
            let Some(symbol) = self.workspace_index.get_symbol(&member.fqn) else {
                continue;
            };
            let Some(origin) = symbol
                .kotlin
                .as_ref()
                .and_then(|kotlin| kotlin.jvm_origin.clone())
            else {
                continue;
            };
            let spelled = symbol.simple_name.to_string();
            let origin_is_property = self
                .workspace_index
                .get_symbol(&origin)
                .is_some_and(|origin| origin.kind == LspSymbolKind::PROPERTY);
            let replacement = if origin_is_property && spelled == old_getter {
                Some(new_getter.clone())
            } else if origin_is_property && spelled == old_setter {
                Some(new_setter.clone())
            } else if spelled == old_name {
                Some(new_name.to_string())
            } else {
                None
            };
            if let Some(replacement) = replacement {
                let key = member.fqn.split('(').next().unwrap_or(&member.fqn);
                spellings.insert(key.to_string(), (spelled, replacement));
            }
        }
        spellings
    }

    /// A declaration without an indexed FQN is lexical by construction. Its
    /// complete rename proof is therefore the selected file revision, not an
    /// O(workspace) scan which can neither strengthen nor correctly bind the
    /// local identity.
    fn collect_local_references(
        &self,
        target: &RenameTarget,
        cancellation: &CancellationToken,
    ) -> Result<RevisionedRenameReferences, String> {
        let indexed = self.indexed_content(target.definition_revision)?;
        let uri = Url::from_file_path(&indexed.file_path).map_err(|()| {
            format!(
                "Cannot rename atomically: '{}' cannot be represented as a file URI",
                indexed.file_path.display()
            )
        })?;
        let mut references = RevisionedRenameReferences::default();
        references.insert_guard(target.definition_revision);
        for range in identifier_ranges(target.language, &indexed.content, &target.token.name) {
            if cancellation.is_cancelled() {
                return Ok(RevisionedRenameReferences::default());
            }
            let location = Location {
                uri: uri.clone(),
                range,
            };
            if location == target.definition
                || self.reference_resolves_to(&location, target.definition_revision, target)?
            {
                references.push(target.definition_revision, location);
            }
        }
        Ok(references)
    }

    fn validate_new_name_for_edit_languages(
        &self,
        references: &RevisionedRenameReferences,
        target_kind: Option<LspSymbolKind>,
        new_name: &str,
    ) -> Result<(), String> {
        let mut checked = std::collections::HashSet::new();
        for reference in &references.locations {
            if !checked.insert(reference.revision) {
                continue;
            }
            let indexed = self.indexed_content(reference.revision)?;
            let language = Language::from_path(&indexed.file_path).ok_or_else(|| {
                format!(
                    "Cannot rename atomically: edit target '{}' has no supported source language",
                    indexed.file_path.display()
                )
            })?;
            if !is_valid_identifier(language, target_kind, new_name) {
                return Err(format!(
                    "'{new_name}' is not a valid {} identifier required by edit target '{}'",
                    language.name(),
                    indexed.file_path.display()
                ));
            }
        }
        Ok(())
    }

    /// Collect one method's complete override-connected declaration family and
    /// every current usage under a single workspace graph revision.
    ///
    /// Textual identifier scanning is insufficient here: an interface method,
    /// an inherited implementation, and calls through several receiver types
    /// must all be renamed together. Any incomplete hierarchy or dependency
    /// declaration makes the refactoring unavailable rather than partial.
    fn collect_callable_references(
        &self,
        target: &RenameTarget,
        new_name: &str,
        cancellation: &CancellationToken,
    ) -> Result<RevisionedRenameReferences, String> {
        let fqn = target.fqn.as_deref().ok_or_else(|| {
            "Cannot rename atomically: callable identity is unavailable".to_string()
        })?;
        let callable_target = SymbolTarget::new(fqn, DeclarationNamespace::Callable);
        let family = match self
            .workspace_index
            .find_current_callable_override_family_for_refactoring_cancellable(
                &callable_target,
                target.workspace_revision,
                || cancellation.is_cancelled(),
            ) {
            Ok(family) => family,
            Err(CurrentUsageQueryError::Cancelled) if cancellation.is_cancelled() => {
                return Ok(RevisionedRenameReferences::default());
            }
            Err(error) => {
                return Err(format!(
                    "Cannot rename atomically: callable family is unavailable: {error}"
                ));
            }
        };
        if family.is_empty()
            || !family.iter().any(|definition| {
                definition.target == callable_target
                    && definition.revision() == Some(target.definition_revision)
            })
        {
            return Err(
                "Cannot rename atomically: selected callable is no longer the elected definition"
                    .to_string(),
            );
        }
        if family
            .iter()
            .any(|definition| definition.revision().is_none())
        {
            return Err(
                "Cannot rename atomically: the callable family contains a dependency declaration"
                    .to_string(),
            );
        }
        let family_targets = family
            .iter()
            .map(|definition| definition.target.clone())
            .collect::<Vec<_>>();
        if new_name != target.token.name
            && self
                .workspace_index
                .current_callable_family_conflicts_with_name_for_refactoring(
                    &family_targets,
                    new_name,
                    target.workspace_revision,
                    || cancellation.is_cancelled(),
                )
                .map_err(|error| {
                    format!(
                        "Cannot rename atomically: callable conflict proof is unavailable: {error}"
                    )
                })?
        {
            return Err(format!(
                "Cannot rename '{}' to '{}': '{}' already exists in the relevant type hierarchy",
                target.token.name, new_name, new_name
            ));
        }
        let usages = match self
            .workspace_index
            .find_current_usages_for_symbol_targets_for_refactoring(
                &family_targets,
                target.workspace_revision,
                &[],
                || cancellation.is_cancelled(),
            ) {
            Ok(usages) => usages,
            Err(CurrentUsageQueryError::Cancelled) if cancellation.is_cancelled() => {
                return Ok(RevisionedRenameReferences::default());
            }
            Err(error) => {
                return Err(format!(
                    "Cannot rename atomically: callable usage provenance is unavailable: {error}"
                ));
            }
        };

        let mut references = RevisionedRenameReferences {
            workspace_revision: Some(target.workspace_revision),
            resource_proof: usages.resource_proof().cloned(),
            ..RevisionedRenameReferences::default()
        };
        for &revision in usages.scanned_revisions() {
            references.insert_guard(revision);
        }
        for definition in family {
            if definition.symbol.simple_name.as_ref() != target.token.name {
                return Err(format!(
                    "Cannot rename atomically: family declaration '{}' has a different source name",
                    definition.target.fqn
                ));
            }
            let revisioned = definition.revisioned_location().ok_or_else(|| {
                "Cannot rename atomically: callable family revision is unavailable".to_string()
            })?;
            let revision = revisioned.revision;
            let location = crate::navigation_util::revisioned_symbol_location_to_lsp_location(
                &self.workspace_index,
                &revisioned,
            )?
            .ok_or_else(|| {
                format!(
                    "Cannot rename atomically: callable declaration '{}' has no current source range",
                    definition.target.fqn
                )
            })?;
            references.push(revision, location);
        }
        for file in &usages.files {
            for location in &file.locations {
                let revisioned = RevisionedLocation {
                    revision: file.revision,
                    location: location.clone(),
                };
                let lsp_location =
                    crate::navigation_util::revisioned_symbol_location_to_lsp_location(
                        &self.workspace_index,
                        &revisioned,
                    )?
                    .ok_or_else(|| {
                        format!(
                            "Cannot rename atomically: callable usage '{}' has no current source range",
                            location.fqn
                        )
                    })?;
                references.push(file.revision, lsp_location);
            }
        }
        if cancellation.is_cancelled() {
            return Ok(RevisionedRenameReferences::default());
        }
        if !self
            .workspace_index
            .current_usage_locations_are_current(&usages)
        {
            return Err(
                "Cannot rename atomically: callable usages changed during the request; try again"
                    .to_string(),
            );
        }
        Ok(references)
    }

    fn require_workspace_revision(&self, expected: WorkspaceRevision) -> Result<(), String> {
        if self.workspace_index.current_workspace_revision() != expected {
            return Err(
                "Cannot rename atomically: workspace bindings changed during the request; try again"
                    .to_string(),
            );
        }
        Ok(())
    }

    fn reference_resolves_to(
        &self,
        reference: &Location,
        revision: FileRevisionToken,
        target: &RenameTarget,
    ) -> Result<bool, String> {
        let path = reference
            .uri
            .to_file_path()
            .map_err(|()| "Cannot rename atomically: reference is not a file URI".to_string())?;
        let indexed = self.indexed_content(revision)?;
        let file_id = revision.file_id();
        if indexed.file_path != path {
            return Err("Cannot rename atomically: reference path changed revisions".to_string());
        }
        let source = &indexed.content;

        let language = Language::from_path(&path).ok_or_else(|| {
            format!(
                "Cannot rename atomically: unsupported reference file '{}'",
                path.display()
            )
        })?;
        if language == Language::Java {
            let reference_offset = position_to_offset(source, reference.range.start)
                .map_err(|error| format!("Cannot rename: invalid reference position: {error}"))?;
            let explicitly_qualified = source[..reference_offset as usize]
                .trim_end()
                .ends_with('.');
            if !explicitly_qualified {
                if let Some(matches) =
                    self.java_lexical_reference_matches(file_id, reference, target, source)?
                {
                    return Ok(matches);
                }
            }
        }

        let exact = ReadTxn::scratch(|db| {
            db.set_file_text_tracked(file_id, Arc::new(source.to_string()));
            db.set_file_language(file_id, language);
        });
        NavigationProvider::new(
            &exact,
            Arc::clone(&self.workspace_index),
            Arc::clone(&self.query_engine),
            Arc::clone(&self.source_providers),
        )
        .goto_definition(file_id, reference.range.start, source, &reference.uri)
        .and_then(|definition| {
            definition.map_or(Ok(false), |definition| {
                self.definition_matches_target(&definition, target)
            })
        })
    }

    fn definition_matches_target(
        &self,
        definition: &Location,
        target: &RenameTarget,
    ) -> Result<bool, String> {
        if definition == &target.definition {
            return Ok(true);
        }
        if !matches!(
            target.kind,
            Some(
                LspSymbolKind::CLASS
                    | LspSymbolKind::INTERFACE
                    | LspSymbolKind::ENUM
                    | LspSymbolKind::STRUCT
            )
        ) {
            return Ok(false);
        }
        let Some(target_fqn) = target.fqn.as_deref() else {
            return Err(
                "Cannot rename atomically: type identity is unavailable for constructor matching"
                    .to_string(),
            );
        };

        for file in self
            .workspace_index
            .find_by_simple_name_revisioned(&target.token.name)
            .files
        {
            for candidate in file.locations {
                if candidate.kind != LspSymbolKind::CONSTRUCTOR
                    || candidate.container_name.as_deref() != Some(target_fqn)
                {
                    continue;
                }
                let revisioned = RevisionedLocation {
                    revision: file.revision,
                    location: candidate,
                };
                let Some(location) =
                    crate::navigation_util::revisioned_symbol_location_to_lsp_location(
                        &self.workspace_index,
                        &revisioned,
                    )?
                else {
                    return Err(
                        "Cannot rename atomically: constructor revision was retired during matching"
                            .to_string(),
                    );
                };
                if &location == definition {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn java_lexical_reference_matches(
        &self,
        file_id: FileId,
        reference: &Location,
        target: &RenameTarget,
        source: &str,
    ) -> Result<Option<bool>, String> {
        let offset = position_to_offset(source, reference.range.start)
            .map_err(|error| format!("Cannot rename: invalid reference start: {error}"))?;
        let symbol_table = java_symbol_table_from_source(file_id, source);
        let resolver = SymbolResolver::with_cached_table(symbol_table);
        let Some(scope) = resolver.find_scope_at_offset(offset) else {
            return Ok(None);
        };
        let Some(resolved) = resolver.resolve_in_scope_at_offset(&target.token.name, scope, offset)
        else {
            return Ok(None);
        };
        let Some(symbol) = resolver.symbol_table().get_symbol(resolved.symbol_id) else {
            return Ok(None);
        };
        let reference_end = position_to_offset(source, reference.range.end)
            .map_err(|error| format!("Cannot rename: invalid reference end: {error}"))?;
        let is_declaration =
            symbol.location.range.start == offset && symbol.location.range.end == reference_end;
        let is_lexical = matches!(
            symbol.kind,
            hyperion_semantics::symbol::SymbolKind::LocalVariable { .. }
                | hyperion_semantics::symbol::SymbolKind::Parameter { .. }
        );
        if !is_declaration && !is_lexical {
            return Ok(None);
        }

        let target_source = self.indexed_content(target.definition_revision)?;
        let target_start =
            position_to_offset(&target_source.content, target.definition.range.start).map_err(
                |error| format!("Cannot rename: invalid target declaration start: {error}"),
            )?;
        let target_end = position_to_offset(&target_source.content, target.definition.range.end)
            .map_err(|error| format!("Cannot rename: invalid target declaration end: {error}"))?;
        Ok(Some(
            symbol.location.file_id == target.definition_file_id
                && symbol.location.range.start == target_start
                && symbol.location.range.end == target_end,
        ))
    }

    fn assemble_workspace_edit(
        &self,
        references: &RevisionedRenameReferences,
        expected_name: &str,
        new_name: &str,
        cancellation: &CancellationToken,
    ) -> Result<Option<WorkspaceEdit>, String> {
        if let Some(workspace_revision) = references.workspace_revision {
            self.require_workspace_revision(workspace_revision)?;
        }
        let mut content_by_revision = HashMap::new();
        for revision in &references.guards {
            if cancellation.is_cancelled() {
                return Ok(None);
            }
            content_by_revision.insert(*revision, self.indexed_content(*revision)?);
        }
        if let Some(workspace_revision) = references.workspace_revision {
            self.require_workspace_revision(workspace_revision)?;
        }

        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        for reference in &references.locations {
            if cancellation.is_cancelled() {
                return Ok(None);
            }
            let path = reference.location.uri.to_file_path().map_err(|()| {
                "Cannot rename atomically: an edit target is not a file URI".to_string()
            })?;
            let indexed = content_by_revision
                .get(&reference.revision)
                .ok_or_else(|| {
                    "Cannot rename atomically: edit revision was not guarded".to_string()
                })?;
            if indexed.file_path != path {
                return Err(
                    "Cannot rename atomically: edit target path changed revisions".to_string(),
                );
            }
            let start = position_to_offset(&indexed.content, reference.location.range.start)
                .map_err(|error| {
                    format!(
                        "Cannot rename atomically: invalid edit start in '{}': {error}",
                        path.display()
                    )
                })? as usize;
            let end = position_to_offset(&indexed.content, reference.location.range.end).map_err(
                |error| {
                    format!(
                        "Cannot rename atomically: invalid edit end in '{}': {error}",
                        path.display()
                    )
                },
            )? as usize;
            let actual = indexed.content.get(start..end).ok_or_else(|| {
                format!(
                    "Cannot rename atomically: invalid edit byte range in '{}'",
                    path.display()
                )
            })?;
            let (expected_here, replacement) = reference
                .spelling
                .as_ref()
                .map_or((expected_name, new_name), |(old, new)| {
                    (old.as_str(), new.as_str())
                });
            if actual != expected_here {
                return Err(format!(
                    "Cannot rename atomically: expected '{}' at edit target '{}', found '{}'",
                    expected_here,
                    path.display(),
                    actual
                ));
            }
            changes
                .entry(reference.location.uri.clone())
                .or_default()
                .push(TextEdit {
                    range: reference.location.range,
                    new_text: replacement.to_string(),
                });
        }
        // Recheck after every edit was validated. This catches a same-text or
        // changed-text publication that retired any guarded range while the
        // multi-file edit was being assembled.
        for revision in &references.guards {
            if self
                .workspace_index
                .get_indexed_revision_content(*revision)
                .is_none()
            {
                return Err(format!(
                    "Cannot rename atomically: indexed revision {:?} was retired during final assembly; try again",
                    revision
                ));
            }
        }
        if let Some(workspace_revision) = references.workspace_revision {
            self.require_workspace_revision(workspace_revision)?;
        }
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }))
    }

    fn resolve_target(
        &self,
        file_id: FileId,
        request_revision: FileRevisionToken,
        position: Position,
        source_text: &str,
        uri: &Url,
    ) -> Result<Option<RenameTarget>, String> {
        let request_path = uri
            .to_file_path()
            .map_err(|()| "Cannot rename a non-workspace document".to_string())?;
        let language = Language::from_path(&request_path)
            .ok_or_else(|| "Cannot rename an unsupported source language".to_string())?;
        let exact = ReadTxn::scratch(|db| {
            db.set_file_text_tracked(file_id, Arc::new(source_text.to_string()));
            db.set_file_language(file_id, language);
            db.set_workspace_index_handle(WorkspaceIndexHandle(Arc::clone(&self.workspace_index)));
            db.set_workspace_version(0);
        });
        let snapshot = exact.snapshot();

        let ctx = crate::language_handler::LanguageHandlerContext {
            txn: &exact,
            workspace_index: &self.workspace_index,
            source_providers: &self.source_providers,
            query_engine: Some(&self.query_engine),
        };
        let token = ctx.dispatch_opt_in(
            language,
            |handler| handler.rename_token(file_id, position, source_text),
            || java_rename_token(snapshot, file_id, position, source_text),
            || {
                Err(format!(
                    "Cannot rename: {} token resolution is not implemented",
                    language.name()
                ))
            },
        )?;
        let Some(token) = token else {
            return Ok(None);
        };

        let workspace_revision = self.workspace_index.current_workspace_revision();
        let token_range = lsp_range_to_text_range(source_text, token.range)?;
        let token_range = hyperion_workspace::TextRange::new(
            u32::from(token_range.start()),
            u32::from(token_range.end()),
        );
        let mut exact_declarations = [
            DeclarationNamespace::Type,
            DeclarationNamespace::Constructor,
            DeclarationNamespace::Callable,
            DeclarationNamespace::Value,
        ]
        .into_iter()
        .filter_map(|namespace| {
            self.workspace_index
                .find_symbol_definition_at_revision_range(
                    request_revision,
                    workspace_revision,
                    token_range,
                    namespace,
                )
        })
        .collect::<Vec<_>>();
        exact_declarations.sort_unstable_by(|left, right| {
            left.target
                .fqn
                .cmp(&right.target.fqn)
                .then((left.target.namespace as u8).cmp(&(right.target.namespace as u8)))
        });
        exact_declarations.dedup_by(|left, right| left.target == right.target);
        if exact_declarations.len() > 1 {
            return Err(format!(
                "Cannot rename '{}': its declaration range is semantically ambiguous",
                token.name
            ));
        }
        let exact_declaration = exact_declarations.pop();
        let (definition, resolved_declaration) = if let Some(exact_declaration) = exact_declaration
        {
            if exact_declaration.symbol.simple_name.as_ref() != token.name {
                return Err(format!(
                    "Cannot rename '{}': its indexed declaration has a different source name",
                    token.name
                ));
            }
            let revisioned = exact_declaration.revisioned_location().ok_or_else(|| {
                format!(
                    "Cannot rename '{}': its declaration is in a dependency",
                    token.name
                )
            })?;
            let location = crate::navigation_util::revisioned_symbol_location_to_lsp_location(
                &self.workspace_index,
                &revisioned,
            )?
            .ok_or_else(|| {
                format!(
                    "Cannot rename '{}': its exact declaration revision is unavailable",
                    token.name
                )
            })?;
            (location, Some(exact_declaration))
        } else {
            let exact_navigation = NavigationProvider::new(
                &exact,
                Arc::clone(&self.workspace_index),
                Arc::clone(&self.query_engine),
                Arc::clone(&self.source_providers),
            );
            if let Some(resolved) = exact_navigation.resolve_workspace_target_at_position(
                file_id,
                position,
                source_text,
                uri,
                None,
            )? {
                if resolved.request_revision != request_revision {
                    return Err(format!(
                        "Cannot rename '{}': its request source changed during identity resolution",
                        token.name
                    ));
                }
                let Some(location) = resolved.location else {
                    return Err(format!(
                        "Cannot rename '{}': its declaration is in a dependency without attached source",
                        token.name
                    ));
                };
                (location, Some(resolved.definition))
            } else {
                let Some(location) =
                    exact_navigation.goto_definition(file_id, position, source_text, uri)?
                else {
                    return Err(format!(
                        "Cannot rename '{}': the symbol does not resolve to a declaration",
                        token.name
                    ));
                };
                if location.uri != *uri
                    || !lexical_declaration_matches(
                        snapshot,
                        language,
                        file_id,
                        source_text,
                        &token.name,
                        location.range,
                    )?
                {
                    return Err(format!(
                        "Cannot rename '{}': navigation did not produce one exact indexed or lexical identity",
                        token.name
                    ));
                }
                (location, None)
            }
        };
        self.require_workspace_revision(workspace_revision)?;

        let definition_path = definition.uri.to_file_path().map_err(|()| {
            format!(
                "Cannot rename '{}': its declaration is outside the workspace",
                token.name
            )
        })?;
        let Some(definition_file_id) = self.workspace_index.get_file_id(&definition_path) else {
            return Err(format!(
                "Cannot rename '{}': its declaration is outside the workspace",
                token.name
            ));
        };
        if self.workspace_index.is_external_file(definition_file_id) {
            return Err(format!(
                "Cannot rename '{}': its declaration is in a dependency",
                token.name
            ));
        }

        let definition_revision = if definition_file_id == file_id {
            request_revision
        } else {
            self.workspace_index
                .current_file_revision(definition_file_id)
                .ok_or_else(|| {
                    format!(
                        "Cannot rename '{}': its declaration revision is unavailable",
                        token.name
                    )
                })?
        };
        if let Some(resolved_declaration) = resolved_declaration.as_ref() {
            if resolved_declaration.workspace_revision != workspace_revision
                || resolved_declaration.revision() != Some(definition_revision)
            {
                return Err(format!(
                    "Cannot rename '{}': its resolved declaration changed revisions",
                    token.name
                ));
            }
        }
        let definition_content = self.indexed_content(definition_revision)?;
        if definition_content.file_path != definition_path {
            return Err(format!(
                "Cannot rename '{}': its declaration path changed revisions",
                token.name
            ));
        }
        let definition_start =
            position_to_offset(&definition_content.content, definition.range.start)
                .map_err(|error| format!("Cannot rename: invalid declaration start: {error}"))?;
        let definition_end = position_to_offset(&definition_content.content, definition.range.end)
            .map_err(|error| format!("Cannot rename: invalid declaration end: {error}"))?;
        if definition_content
            .content
            .get(definition_start as usize..definition_end as usize)
            != Some(token.name.as_str())
        {
            if resolved_declaration.as_ref().is_some_and(|resolved| {
                !self
                    .workspace_index
                    .jvm_aliases_of(&resolved.target.fqn)
                    .is_empty()
            }) {
                return Err(format!(
                    "Cannot rename '{}': it is the Java-visible form (accessor, file facade \
                     function or static forwarder) of a Kotlin declaration; rename the Kotlin \
                     declaration instead",
                    token.name
                ));
            }
            return Err(format!(
                "Cannot rename '{}': its declaration range is not bound to the exact indexed revision",
                token.name
            ));
        }

        if resolved_declaration
            .as_ref()
            .is_some_and(|definition| definition.symbol.kind == LspSymbolKind::CONSTRUCTOR)
        {
            let owner_fqn = resolved_declaration
                .as_ref()
                .expect("constructor identity checked above")
                .symbol
                .container
                .as_deref()
                .ok_or_else(|| {
                    "Cannot rename a constructor without its owning type identity".to_string()
                })?;
            let owner = self
                .workspace_index
                .find_symbol_definition(&SymbolTarget::new(owner_fqn, DeclarationNamespace::Type))
                .ok_or_else(|| {
                    format!(
                        "Cannot rename constructor '{}': owning type '{owner_fqn}' is unavailable",
                        token.name
                    )
                })?;
            if owner.workspace_revision != workspace_revision
                || owner.symbol.simple_name.as_ref() != token.name
            {
                return Err(format!(
                    "Cannot rename constructor '{}': its owning type changed during resolution",
                    token.name
                ));
            }
            let owner_revision = owner.revision().ok_or_else(|| {
                format!(
                    "Cannot rename constructor '{}': its owning type is in a dependency",
                    token.name
                )
            })?;
            let owner_path = self
                .workspace_index
                .get_file_path(owner_revision.file_id())
                .ok_or_else(|| {
                    format!(
                        "Cannot rename constructor '{}': owning source path is unavailable",
                        token.name
                    )
                })?;
            let owner_language = Language::from_path(&owner_path).ok_or_else(|| {
                format!(
                    "Cannot rename constructor '{}': owning source language is unsupported",
                    token.name
                )
            })?;
            let owner_location =
                crate::navigation_util::revisioned_symbol_location_to_lsp_location(
                    &self.workspace_index,
                    &owner.revisioned_location().ok_or_else(|| {
                        "Cannot rename constructor: owning type revision is unavailable".to_string()
                    })?,
                )?
                .ok_or_else(|| {
                    "Cannot rename constructor: owning type source range is unavailable".to_string()
                })?;
            self.require_workspace_revision(workspace_revision)?;
            return Ok(Some(RenameTarget {
                token,
                definition: owner_location,
                definition_file_id: owner_revision.file_id(),
                definition_revision: owner_revision,
                workspace_revision,
                fqn: Some(owner.target.fqn.to_string()),
                kind: Some(owner.symbol.kind),
                language: owner_language,
            }));
        }
        let fqn = resolved_declaration
            .as_ref()
            .map(|definition| definition.target.fqn.to_string());
        let kind = resolved_declaration
            .as_ref()
            .map(|definition| definition.symbol.kind);
        let definition_language = Language::from_path(&definition_path).ok_or_else(|| {
            format!(
                "Cannot rename '{}': its declaration language is unsupported",
                token.name
            )
        })?;

        self.require_workspace_revision(workspace_revision)?;

        Ok(Some(RenameTarget {
            token,
            definition,
            definition_file_id,
            definition_revision,
            workspace_revision,
            fqn,
            kind,
            language: definition_language,
        }))
    }
}

fn file_rename_destination_collides(old_path: &Path, new_path: &Path) -> std::io::Result<bool> {
    if old_path == new_path {
        return Ok(false);
    }
    let new_metadata = match std::fs::symlink_metadata(new_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if new_metadata.file_type().is_symlink() {
        return Ok(true);
    }
    // A case-only path change aliases the old file on case-insensitive file
    // systems. Treat that as the source itself, while real files, hard links,
    // and symlink entries remain collisions.
    Ok(std::fs::canonicalize(old_path)? != std::fs::canonicalize(new_path)?)
}

fn file_rename_plan_is_current(plan: &FileRenamePlan) -> bool {
    let (Ok(old_path), Ok(new_path)) = (plan.old_uri.to_file_path(), plan.new_uri.to_file_path())
    else {
        return false;
    };
    std::fs::symlink_metadata(&old_path).is_ok_and(|metadata| metadata.file_type().is_file())
        && file_rename_destination_collides(&old_path, &new_path).is_ok_and(|collides| !collides)
}

fn java_symbol_table_from_source(
    file_id: FileId,
    source: &str,
) -> Arc<hyperion_semantics::symbol::SymbolTable> {
    ReadTxn::scratch(|db| {
        db.set_file_text_tracked(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Java);
    })
    .file_symbol_table(file_id)
}

/// Prove that a navigation-only declaration is a lexical binding rather than
/// an indexed declaration whose identity bridge failed. This is the only
/// location-only identity accepted by rename; its exact declaration range is
/// still fenced by the request file revision.
fn lexical_declaration_matches(
    snapshot: &salsa::Snapshot<Database>,
    language: Language,
    file_id: FileId,
    source: &str,
    name: &str,
    range: Range,
) -> Result<bool, String> {
    let range = lsp_range_to_text_range(source, range)?;
    let start = u32::from(range.start());
    let end = u32::from(range.end());

    match language {
        Language::Java => {
            let symbol_table = snapshot.file_symbol_table(file_id);
            let mut resolver = SymbolResolver::with_cached_table(Arc::clone(&symbol_table));
            let Some(scope) = resolver.find_scope_at_offset(start) else {
                return Ok(false);
            };
            let Some(resolved) = resolver.resolve_in_scope(name, scope) else {
                return Ok(false);
            };
            let Some(symbol) = symbol_table.get_symbol(resolved.symbol_id) else {
                return Ok(false);
            };
            Ok(matches!(
                symbol.kind,
                hyperion_semantics::symbol::SymbolKind::LocalVariable { .. }
                    | hyperion_semantics::symbol::SymbolKind::Parameter { .. }
                    | hyperion_semantics::symbol::SymbolKind::TypeParameter { .. }
            ) && symbol.location.file_id == file_id
                && symbol.location.range == hyperion_workspace::TextRange::new(start, end))
        }
        Language::Kotlin => {
            let scope_graph = snapshot.scope_graph(file_id);
            Ok(scope_graph
                .resolve_symbol(name, start as usize)
                .is_some_and(|symbol| symbol.definition_range == range))
        }
        Language::Groovy | Language::Scala => Ok(false),
    }
}

fn symbol_scope_key(symbol: &hyperion_workspace::Symbol) -> String {
    symbol.container.as_ref().map_or_else(
        || {
            symbol
                .fqn
                .rsplit_once('.')
                .map_or_else(String::new, |(package, _)| package.to_string())
        },
        ToString::to_string,
    )
}

fn lsp_range_to_text_range(source: &str, range: Range) -> Result<rowan::TextRange, String> {
    let start = position_to_offset(source, range.start)
        .map_err(|error| format!("invalid range start: {error}"))?;
    let end = position_to_offset(source, range.end)
        .map_err(|error| format!("invalid range end: {error}"))?;
    if start > end {
        return Err("invalid range: start is after end".to_string());
    }
    Ok(rowan::TextRange::new(
        rowan::TextSize::from(start),
        rowan::TextSize::from(end),
    ))
}

fn identifier_ranges(language: Language, source: &str, name: &str) -> Vec<Range> {
    let lines = LineIndex::new(source);
    match language {
        Language::Java => {
            let tokens = hyperion_parser::Lexer::new(source).collect();
            let (green, _) = hyperion_parser::Parser::parse(tokens);
            let root = hyperion_parser::SyntaxNode::new_root(green);
            root.descendants_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| {
                    token.kind().is_identifier()
                        && token.text() == name
                        && !is_contextual_keyword(token)
                })
                .map(|token| lines.text_range_to_lsp_range(source, token.text_range()))
                .collect()
        }
        Language::Kotlin => {
            let tokens = hyperion_kotlin::Lexer::new(source).collect();
            let (green, _) = hyperion_kotlin::Parser::parse(tokens);
            let root = hyperion_kotlin::SyntaxNode::new_root(green);
            root.descendants_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| {
                    token.kind() == hyperion_kotlin::SyntaxKind::IDENT && token.text() == name
                })
                .filter(|token| {
                    !crate::navigation::ast_queries::is_in_comment_or_string_kotlin(
                        &root,
                        token.text_range().start().into(),
                    )
                })
                .map(|token| lines.text_range_to_lsp_range(source, token.text_range()))
                .collect()
        }
        Language::Groovy | Language::Scala => Vec::new(),
    }
}

fn java_rename_token(
    snapshot: &salsa::Snapshot<Database>,
    file_id: FileId,
    position: Position,
    source_text: &str,
) -> Result<Option<RenameToken>, String> {
    let offset = position_to_offset(source_text, position)?;
    let parse_tree = snapshot.parse_file(file_id);
    if crate::navigation::ast_queries::is_in_comment_or_string_java(&parse_tree.syntax(), offset) {
        return Ok(None);
    }
    let Some(token) = parse_tree
        .syntax()
        .token_at_offset(rowan::TextSize::from(offset))
        .right_biased()
    else {
        return Ok(None);
    };

    if !token.kind().is_identifier() {
        return Ok(None);
    }
    let name = token.text();
    if JAVA_KEYWORDS.contains(&name) || is_contextual_keyword(&token) {
        return Err(format!("Cannot rename keyword '{name}'"));
    }

    Ok(Some(RenameToken {
        name: name.to_string(),
        range: Range::new(
            offset_to_position(source_text, token.text_range().start().into()),
            offset_to_position(source_text, token.text_range().end().into()),
        ),
    }))
}

pub(crate) fn kotlin_rename_token(
    txn: &ReadTxn,
    file_id: FileId,
    position: Position,
    source_text: &str,
) -> Result<Option<RenameToken>, String> {
    let offset = position_to_offset(source_text, position)?;
    let parse_tree = txn.parse_file(file_id);
    let root = hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());
    if crate::navigation::ast_queries::is_in_comment_or_string_kotlin(&root, offset) {
        return Ok(None);
    }
    let Some(token) = root
        .token_at_offset(rowan::TextSize::from(offset))
        .right_biased()
    else {
        return Ok(None);
    };
    if token.kind() != hyperion_kotlin::SyntaxKind::IDENT {
        return Ok(None);
    }

    Ok(Some(RenameToken {
        name: token.text().to_string(),
        range: Range::new(
            offset_to_position(source_text, token.text_range().start().into()),
            offset_to_position(source_text, token.text_range().end().into()),
        ),
    }))
}

/// Convert LSP Position to byte offset
///
/// BUG FIX: Previously this function incorrectly counted UTF-8 chars instead of
/// UTF-16 code units for the character position. Now delegates to the correct
/// implementation in position_utils which properly handles UTF-16.
fn position_to_offset(
    source: &str,
    position: Position,
) -> Result<u32, crate::position_utils::PositionError> {
    crate::position_utils::position_to_offset_u32(source, position)
}

/// Convert byte offset to LSP Position
///
/// Delegates to crate::position_utils::offset_to_position for correct UTF-16 handling.
fn offset_to_position(source: &str, offset: usize) -> Position {
    crate::position_utils::offset_to_position(source, offset)
}

/// Check if an identifier is actually a contextual keyword in Java
///
/// Java has several contextual keywords that are treated as keywords only in specific
/// contexts (e.g., "var", "yield", "record", "sealed", "permits").
/// This function checks if the given identifier acts as a keyword in its current context.
fn is_contextual_keyword(
    token: &rowan::SyntaxToken<hyperion_parser::syntax::JavaLanguage>,
) -> bool {
    use hyperion_parser::syntax::SyntaxKind;

    let Some(parent) = token.parent() else {
        return true;
    };
    match token.kind() {
        // These tests deliberately use the grammar node, not merely the token
        // spelling. Every restricted word remains a legal identifier in other
        // positions (for example `var var = value` and `int sealed`).
        SyntaxKind::VAR_KW => parent.kind() == SyntaxKind::VAR_TYPE,
        SyntaxKind::YIELD_KW => parent.kind() == SyntaxKind::YIELD_STMT,
        SyntaxKind::RECORD_KW => parent.kind() == SyntaxKind::RECORD_DECL,
        SyntaxKind::SEALED_KW | SyntaxKind::NON_SEALED_KW => parent.kind() == SyntaxKind::MODIFIER,
        SyntaxKind::PERMITS_KW => parent.kind().is_type_declaration(),
        SyntaxKind::WHEN_KW => parent.kind() == SyntaxKind::WHEN_CLAUSE,
        // The parser currently leaves module-info constructs flat at the source
        // root. They are not renameable declarations; reject their grammar
        // tokens conservatively while still accepting these spellings inside
        // ordinary Java declaration/expression nodes.
        SyntaxKind::MODULE_KW
        | SyntaxKind::REQUIRES_KW
        | SyntaxKind::EXPORTS_KW
        | SyntaxKind::OPENS_KW
        | SyntaxKind::TO_KW
        | SyntaxKind::USES_KW
        | SyntaxKind::PROVIDES_KW
        | SyntaxKind::WITH_KW
        | SyntaxKind::TRANSITIVE_KW
        | SyntaxKind::OPEN_KW => parent.kind() == SyntaxKind::SOURCE_FILE,
        _ => false,
    }
}

/// Java reserved keywords that cannot be used as identifiers
const JAVA_KEYWORDS: &[&str] = &[
    // Primitive types
    "boolean",
    "byte",
    "char",
    "double",
    "float",
    "int",
    "long",
    "short",
    // Modifiers
    "abstract",
    "final",
    "native",
    "private",
    "protected",
    "public",
    "static",
    "strictfp",
    "synchronized",
    "transient",
    "volatile",
    // Control flow
    "if",
    "else",
    "switch",
    "case",
    "default",
    "while",
    "do",
    "for",
    "break",
    "continue",
    "return",
    "throw",
    "try",
    "catch",
    "finally",
    // Class-related
    "class",
    "interface",
    "enum",
    "extends",
    "implements",
    "new",
    "instanceof",
    "package",
    "import",
    // Other
    "assert",
    "const",
    "goto",
    "void",
    "throws",
    // Literals
    "null",
    "true",
    "false",
    // Special
    "this",
    "super",
    // `_` has been a keyword since Java 9. Java's module, record, sealed,
    // `var`, and `yield` words are deliberately absent: they are restricted
    // or contextual identifiers and remain legal in declaration contexts
    // where the grammar is unambiguous.
    "_",
];

/// Check whether a string is safe to introduce as a Java identifier.
///
/// The character predicates follow `Character.isJavaIdentifierStart/Part`.
/// Rename additionally rejects identifier-ignorable characters because Java
/// erases them for name equality while Hyperion and source filenames retain
/// their raw spelling.
///
/// A safe rename identifier:
/// - Is not empty
/// - Starts with a Java identifier start character
/// - Contains only Java identifier characters and no identifier-ignorables
/// - Is not a reserved keyword
fn is_valid_java_identifier(name: &str, kind: Option<LspSymbolKind>) -> bool {
    if name.is_empty() {
        return false;
    }

    // Java erases identifier-ignorable characters when comparing names. The
    // workspace index and Java's public-type filename rule both retain raw
    // source spelling, so accepting one here could bypass conflict checks
    // (`foo` versus `f\u{200c}oo`) or even disguise a keyword
    // (`cl\u{200c}ass`). Fail closed until identity is canonicalized across
    // every index, reference, and file-operation boundary.
    if name.chars().any(is_java_identifier_ignorable) {
        return false;
    }

    let mut chars = name.chars();

    // First character must be a valid identifier start
    let first = chars.next().unwrap();
    if !is_java_identifier_start(first) {
        return false;
    }

    // Remaining characters must be valid identifier parts
    for ch in chars {
        if !is_java_identifier_part(ch) {
            return false;
        }
    }

    // Check against reserved keywords
    if JAVA_KEYWORDS.contains(&name) {
        return false;
    }

    // Restricted identifiers are not blanket keywords. Applying their JLS
    // restrictions to the declaration being renamed preserves useful names
    // such as `module` and `get`, without manufacturing syntax errors where
    // the word has a special grammar role.
    let namespace = kind.map(DeclarationNamespace::for_kind);
    let requires_type_identifier = namespace == Some(DeclarationNamespace::Type)
        || kind == Some(LspSymbolKind::TYPE_PARAMETER);
    if requires_type_identifier && matches!(name, "var" | "yield" | "record" | "sealed" | "permits")
    {
        return false;
    }
    // An unqualified invocation whose name is `yield` is parsed as the yield
    // statement in switch-expression-capable Java. A callable rename cannot
    // guarantee that every edited invocation is qualified.
    if namespace == Some(DeclarationNamespace::Callable) && name == "yield" {
        return false;
    }

    true
}

const KOTLIN_KEYWORDS: &[&str] = &[
    "as",
    "break",
    "class",
    "continue",
    "do",
    "else",
    "false",
    "for",
    "fun",
    "if",
    "in",
    "interface",
    "is",
    "null",
    "object",
    "package",
    "return",
    "super",
    "this",
    "throw",
    "true",
    "try",
    "typealias",
    "typeof",
    "val",
    "var",
    "when",
    "while",
    // Kotlin soft keywords (`by`, `get`, `set`, `field`, `where`, ...)
    // remain valid identifiers outside their contextual grammar positions.
];

const GROOVY_KEYWORDS: &[&str] = &[
    "as",
    "assert",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "def",
    "default",
    "do",
    "else",
    "enum",
    "extends",
    "false",
    "finally",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "native",
    "new",
    "non-sealed",
    "null",
    "package",
    "permits",
    "private",
    "protected",
    "public",
    "record",
    "return",
    "sealed",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "trait",
    "transient",
    "true",
    "try",
    "var",
    "void",
    "volatile",
    "while",
    "yield",
];

const SCALA_KEYWORDS: &[&str] = &[
    "abstract",
    "case",
    "catch",
    "class",
    "def",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "final",
    "finally",
    "for",
    "forSome",
    "given",
    "if",
    "implicit",
    "import",
    "lazy",
    "match",
    "new",
    "null",
    "object",
    "opaque",
    "open",
    "override",
    "package",
    "private",
    "protected",
    "return",
    "sealed",
    "super",
    "then",
    "this",
    "throw",
    "trait",
    "transparent",
    "true",
    "try",
    "type",
    "using",
    "val",
    "var",
    "while",
    "with",
    "yield",
];

fn is_valid_plain_identifier(name: &str, keywords: &[&str]) -> bool {
    if name.is_empty() || keywords.contains(&name) {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (unicode_ident::is_xid_start(first) || matches!(first, '_' | '$'))
        && chars.all(|character| {
            unicode_ident::is_xid_continue(character) || matches!(character, '_' | '$')
        })
}

fn is_valid_identifier(language: Language, target_kind: Option<LspSymbolKind>, name: &str) -> bool {
    match language {
        Language::Java => is_valid_java_identifier(name, target_kind),
        Language::Kotlin => {
            if name.is_empty() || KOTLIN_KEYWORDS.contains(&name) {
                return false;
            }
            let mut chars = name.chars();
            let Some(first) = chars.next() else {
                return false;
            };
            (unicode_ident::is_xid_start(first) || first == '_')
                && chars.all(|ch| unicode_ident::is_xid_continue(ch) || ch == '_')
        }
        Language::Groovy => is_valid_plain_identifier(name, GROOVY_KEYWORDS),
        Language::Scala => is_valid_plain_identifier(name, SCALA_KEYWORDS),
    }
}

/// Check if a character can start a Java identifier
fn is_java_identifier_start(ch: char) -> bool {
    use unicode_general_category::{get_general_category, GeneralCategory};

    matches!(
        get_general_category(ch),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::LetterNumber
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ConnectorPunctuation
    )
}

/// Check if a character can be part of a Java identifier (not the start)
fn is_java_identifier_part(ch: char) -> bool {
    use unicode_general_category::{get_general_category, GeneralCategory};

    is_java_identifier_start(ch)
        || matches!(
            get_general_category(ch),
            GeneralCategory::NonspacingMark
                | GeneralCategory::SpacingMark
                | GeneralCategory::DecimalNumber
        )
        || is_java_identifier_ignorable(ch)
}

/// Java additionally permits identifier-ignorable ISO controls and Unicode
/// formatting characters in non-leading positions.
fn is_java_identifier_ignorable(ch: char) -> bool {
    use unicode_general_category::{get_general_category, GeneralCategory};

    matches!(ch, '\u{0000}'..='\u{0008}' | '\u{000E}'..='\u{001B}' | '\u{007F}'..='\u{009F}')
        || get_general_category(ch) == GeneralCategory::Format
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_semantics::db::{Database, Language, SourceDatabase};
    use std::sync::Arc;

    fn create_test_provider(source: &str) -> (RenameProvider, FileId, Url) {
        // Create workspace index and query engine
        let workspace_index = WorkspaceIndex::new(std::env::temp_dir());
        let file_path = std::env::temp_dir().join("hyperion-rename-provider-test.java");
        workspace_index
            .index_file_with_source(&file_path, source)
            .expect("test source should index");
        let file_id = workspace_index
            .get_file_id(&file_path)
            .expect("test source should have a file id");
        let uri = Url::from_file_path(file_path).expect("test path should be a URI");
        let query_engine = QueryEngine::new(workspace_index.clone());

        let workspace_index_arc = Arc::new(workspace_index);
        let query_engine_arc = Arc::new(query_engine);

        let source_providers = Arc::new(crate::source_provider::SourceProviderChain::new());
        let provider = RenameProvider::new(
            Arc::clone(&workspace_index_arc),
            Arc::clone(&query_engine_arc),
            source_providers,
        );

        (provider, file_id, uri)
    }

    fn create_isolated_test_provider(
        source: &str,
        extension: &str,
        _language: Language,
    ) -> (tempfile::TempDir, RenameProvider, FileId, Url) {
        let temp = tempfile::tempdir().expect("tempdir");
        let file_path = temp.path().join(format!("Target.{extension}"));
        let workspace_index = WorkspaceIndex::new(temp.path().to_path_buf());
        workspace_index
            .index_file_with_source(&file_path, source)
            .expect("test source should index");
        let file_id = workspace_index
            .get_file_id(&file_path)
            .expect("test source should have a file id");
        let uri = Url::from_file_path(&file_path).expect("test path should be a URI");
        let query_engine = QueryEngine::new(workspace_index.clone());
        let provider = RenameProvider::new(
            Arc::new(workspace_index),
            Arc::new(query_engine),
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );
        (temp, provider, file_id, uri)
    }

    fn revisioned_reference(
        provider: &RenameProvider,
        file_id: FileId,
        uri: &Url,
        source: &str,
        name: &str,
    ) -> (FileRevisionToken, Location) {
        let revision = provider
            .workspace_index
            .current_file_revision(file_id)
            .expect("published revision");
        let start = source.find(name).expect("name offset");
        let end = start + name.len();
        let lines = LineIndex::new(source);
        (
            revision,
            Location {
                uri: uri.clone(),
                range: Range::new(
                    lines.offset_to_position(source, start),
                    lines.offset_to_position(source, end),
                ),
            },
        )
    }

    #[test]
    fn public_top_level_java_type_carries_exact_file_rename_plan() {
        let source = "public class Target {}";
        let (_temp, provider, file_id, uri) =
            create_isolated_test_provider(source, "java", Language::Java);
        let old_path = uri.to_file_path().expect("old file path");
        std::fs::write(&old_path, source).expect("materialized source file");
        let position = LineIndex::new(source)
            .offset_to_position(source, source.find("Target").expect("type name"));

        let result = provider
            .rename(file_id, position, "Renamed", source, &uri)
            .expect("rename")
            .expect("rename available");
        assert_eq!(
            result.file_rename,
            Some(FileRenamePlan {
                old_uri: uri,
                new_uri: Url::from_file_path(old_path.with_file_name("Renamed.java"))
                    .expect("new file URI"),
            })
        );
    }

    #[test]
    fn constructor_origin_rename_canonicalizes_to_type_and_file() {
        let source = "public class Target { Target() {} void use() { new Target(); } }";
        let (_temp, provider, file_id, uri) =
            create_isolated_test_provider(source, "java", Language::Java);
        let old_path = uri.to_file_path().expect("old file path");
        std::fs::write(&old_path, source).expect("materialized source file");
        let constructor_offset = source.find("Target() {}").expect("constructor name");
        let position = LineIndex::new(source).offset_to_position(source, constructor_offset);

        let result = provider
            .rename(file_id, position, "Renamed", source, &uri)
            .expect("rename")
            .expect("rename available");
        assert_eq!(
            result.file_rename,
            Some(FileRenamePlan {
                old_uri: uri.clone(),
                new_uri: Url::from_file_path(old_path.with_file_name("Renamed.java"))
                    .expect("new file URI"),
            })
        );
        let mut changes = result.edit.changes.expect("plain edit");
        let edits = changes.remove(&uri).expect("source edits");
        assert_eq!(edits.len(), 3, "type, constructor, and construction site");
        assert!(edits.iter().all(|edit| edit.new_text == "Renamed"));
    }

    #[test]
    fn chained_generic_call_and_declaration_share_one_rename_identity() {
        let temp = tempfile::tempdir().expect("tempdir");
        let sources = [
            (
                "java/lang/Object.java",
                "package java.lang; public class Object {}",
            ),
            (
                "Service.java",
                "package p; public class Service { public void target() {} }",
            ),
            (
                "Distractor.java",
                "package p; public class Distractor { public void target() {} }",
            ),
            (
                "Factory.java",
                concat!(
                    "package p; public class Factory<T> { public T service() { ",
                    "return null; } }",
                ),
            ),
            (
                "Use.java",
                concat!(
                    "package p; public class Use { void invoke(Factory<Service> factory) { ",
                    "factory.service().target(); } }",
                ),
            ),
        ];
        let workspace_index = WorkspaceIndex::new(temp.path().to_path_buf());
        let mut files = Vec::new();
        for (name, source) in sources {
            let path = temp.path().join(name);
            std::fs::create_dir_all(path.parent().expect("source parent"))
                .expect("source directory");
            std::fs::write(&path, source).expect("materialized source");
            workspace_index
                .index_file_with_source(&path, source)
                .expect("indexed source");
            files.push((
                source,
                workspace_index.get_file_id(&path).expect("file id"),
                Url::from_file_path(path).expect("file URI"),
            ));
        }
        let workspace_index = Arc::new(workspace_index);
        let query_engine = Arc::new(QueryEngine::new(workspace_index.as_ref().clone()));
        let provider = RenameProvider::new(
            workspace_index,
            query_engine,
            Arc::new(crate::source_provider::SourceProviderChain::new()),
        );

        let rename_at = |file_index: usize, offset: usize| {
            let (source, file_id, uri) = &files[file_index];
            let position = LineIndex::new(source).offset_to_position(source, offset);
            provider
                .rename(*file_id, position, "renamed", source, uri)
                .expect("rename request")
                .expect("rename available")
                .edit
                .changes
                .expect("plain workspace edit")
        };
        let declaration_changes = rename_at(1, files[1].0.find("target").expect("declaration"));
        let call_changes = rename_at(4, files[4].0.rfind("target").expect("call"));

        assert_eq!(call_changes, declaration_changes);
        assert_eq!(
            call_changes.len(),
            2,
            "declaration and exact call files only"
        );
        assert_eq!(call_changes.get(&files[1].2).map(Vec::len), Some(1));
        assert_eq!(call_changes.get(&files[4].2).map(Vec::len), Some(1));
        assert!(!call_changes.contains_key(&files[2].2));
    }

    #[test]
    fn public_top_level_java_type_rejects_existing_target_file() {
        let source = "public class Target {}";
        let (_temp, provider, file_id, uri) =
            create_isolated_test_provider(source, "java", Language::Java);
        let old_path = uri.to_file_path().expect("old file path");
        std::fs::write(&old_path, source).expect("materialized source file");
        std::fs::write(old_path.with_file_name("Renamed.java"), "class Occupied {}")
            .expect("occupied target");
        let position = LineIndex::new(source)
            .offset_to_position(source, source.find("Target").expect("type name"));

        let error = provider
            .rename_proof_cancellable_with_workspace_completeness(
                file_id,
                position,
                "Renamed",
                source,
                &uri,
                true,
                &CancellationToken::default(),
            )
            .expect_err("existing destination must reject the entire rename");
        assert!(error.contains("already exists"), "{error}");
    }

    #[test]
    fn public_nested_java_type_does_not_rename_the_enclosing_source_file() {
        let source = "class Outer { public class Target {} }";
        let (_temp, provider, file_id, uri) =
            create_isolated_test_provider(source, "java", Language::Java);
        let old_path = uri.to_file_path().expect("old file path");
        std::fs::write(&old_path, source).expect("materialized source file");
        let position = LineIndex::new(source)
            .offset_to_position(source, source.find("Target").expect("nested type name"));

        let proof = provider
            .rename_proof_cancellable_with_workspace_completeness(
                file_id,
                position,
                "Renamed",
                source,
                &uri,
                true,
                &CancellationToken::default(),
            )
            .expect("rename proof")
            .expect("rename available");
        assert_eq!(proof.file_rename, None);
    }

    #[test]
    fn file_rename_plan_currentness_fences_late_destination_creation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let old_path = temp.path().join("Target.java");
        let new_path = temp.path().join("Renamed.java");
        std::fs::write(&old_path, "public class Target {}").expect("materialized source file");
        let plan = FileRenamePlan {
            old_uri: Url::from_file_path(&old_path).expect("old URI"),
            new_uri: Url::from_file_path(&new_path).expect("new URI"),
        };
        assert!(file_rename_plan_is_current(&plan));
        std::fs::write(&new_path, "class Occupied {}").expect("late destination creation");
        assert!(!file_rename_plan_is_current(&plan));
    }

    #[test]
    fn final_rename_assembly_rejects_same_text_republication() {
        let source = "class Target {}";
        let (_temp, provider, file_id, uri) =
            create_isolated_test_provider(source, "java", Language::Java);
        let (r1, location) = revisioned_reference(&provider, file_id, &uri, source, "Target");
        let mut references = RevisionedRenameReferences::default();
        references.push(r1, location);

        let path = uri.to_file_path().expect("target path");
        provider
            .workspace_index
            .index_file_with_source(&path, source)
            .expect("same-text R2 publication");

        let error = provider
            .assemble_workspace_edit(
                &references,
                "Target",
                "Renamed",
                &CancellationToken::default(),
            )
            .expect_err("R1 ranges must be rejected after same-text R2");
        assert!(
            error.contains("indexed revision"),
            "unexpected rename rejection: {error}"
        );
    }

    #[test]
    fn multi_file_rename_rejects_changed_publication_after_query() {
        let target_source = "class Target {}";
        let (_temp, provider, target_id, target_uri) =
            create_isolated_test_provider(target_source, "java", Language::Java);
        let dependent_source = "class Use { Target field; }";
        let dependent_path = target_uri
            .to_file_path()
            .expect("target path")
            .parent()
            .unwrap()
            .join("Use.java");
        provider
            .workspace_index
            .index_file_with_source(&dependent_path, dependent_source)
            .expect("dependent R1");
        let dependent_id = provider
            .workspace_index
            .get_file_id(&dependent_path)
            .expect("dependent id");
        let dependent_uri = Url::from_file_path(&dependent_path).expect("dependent URI");

        let (target_r1, target_location) =
            revisioned_reference(&provider, target_id, &target_uri, target_source, "Target");
        let (dependent_r1, dependent_location) = revisioned_reference(
            &provider,
            dependent_id,
            &dependent_uri,
            dependent_source,
            "Target",
        );
        let mut references = RevisionedRenameReferences::default();
        references.push(target_r1, target_location);
        references.push(dependent_r1, dependent_location);

        provider
            .workspace_index
            .index_file_with_source(&dependent_path, "class Use { Object field; }")
            .expect("changed R2 publication");

        let error = provider
            .assemble_workspace_edit(
                &references,
                "Target",
                "Renamed",
                &CancellationToken::default(),
            )
            .expect_err("one retired file token must abort the whole edit");
        assert!(error.contains("indexed revision"));
    }

    #[test]
    fn rename_rejects_evicted_caller_owned_revision() {
        // Kotlin has no Java parse-cache source fallback, so clearing the
        // content LRU makes this editor-only revision genuinely unavailable.
        let source = "class Target";
        let (_temp, provider, file_id, uri) =
            create_isolated_test_provider(source, "kt", Language::Kotlin);
        let path = uri.to_file_path().expect("file path");
        std::fs::write(&path, "class DiskRevision").expect("different disk revision");
        provider.workspace_index.clear_file_cache();

        let error = provider
            .rename(file_id, Position::new(0, 6), "Renamed", source, &uri)
            .expect_err("rename must fail closed without exact editor bytes");
        assert!(error.contains("indexed revision"));
    }

    #[test]
    fn foreign_rename_token_resolution_does_not_fall_through_java() {
        for (extension, language) in [("groovy", Language::Groovy), ("scala", Language::Scala)] {
            let source = "class Target {}";
            let (_temp, provider, file_id, uri) =
                create_isolated_test_provider(source, extension, language);

            let error = provider
                .prepare_rename(file_id, Position::new(0, 6), source, &uri)
                .expect_err("unsupported foreign rename must fail before Java parsing");
            assert!(
                error.contains("token resolution is not implemented"),
                "unexpected {language:?} error: {error}"
            );
        }
    }

    #[test]
    fn rename_aborts_whole_edit_when_any_workspace_revision_is_unavailable() {
        let source = "class Target {}";
        let (_temp, provider, file_id, uri) =
            create_isolated_test_provider(source, "java", Language::Java);
        let target_path = uri.to_file_path().expect("target path");
        std::fs::write(&target_path, source).expect("matching target disk revision");

        // Use Kotlin for the unavailable dependent so the Java parse cache
        // cannot accidentally keep its caller-owned source alive.
        let dependent_source = "class Use { val field: Target? = null }";
        let dependent_path = target_path.parent().unwrap().join("Use.kt");
        provider
            .workspace_index
            .index_file_with_source(&dependent_path, dependent_source)
            .expect("dependent source should index");
        let _dependent_file_id = provider
            .workspace_index
            .get_file_id(&dependent_path)
            .expect("dependent file id");
        std::fs::write(&dependent_path, "class Use").expect("different dependent disk revision");
        provider.workspace_index.clear_file_cache();

        let error = provider
            .rename(file_id, Position::new(0, 6), "Renamed", source, &uri)
            .expect_err("one unavailable revision must abort the entire workspace edit");
        assert!(
            error.contains("exact indexed bytes"),
            "unexpected rename rejection: {error}"
        );
    }

    #[test]
    fn test_prepare_rename_valid_identifier() {
        let source = "class Foo { int myVar; }";
        let (provider, file_id, uri) = create_test_provider(source);

        // Position on "myVar"
        let position = Position::new(0, 16);
        let result = provider.prepare_rename(file_id, position, source, &uri);

        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_prepare_rename_class_name() {
        let source = "class MyClass { }";
        let (provider, file_id, uri) = create_test_provider(source);

        // Position on "MyClass"
        let position = Position::new(0, 6);
        let result = provider.prepare_rename(file_id, position, source, &uri);

        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_prepare_rename_method_name() {
        let source = "class Foo { void myMethod() {} }";
        let (provider, file_id, uri) = create_test_provider(source);

        // Position on "myMethod"
        let position = Position::new(0, 17);
        let result = provider.prepare_rename(file_id, position, source, &uri);

        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn test_prepare_rename_rejects_non_identifier() {
        let source = "class Foo { int x = 5; }";
        let (provider, file_id, uri) = create_test_provider(source);

        // Position on the number "5"
        let position = Position::new(0, 20);
        let result = provider.prepare_rename(file_id, position, source, &uri);

        assert!(result.is_ok());
        // Should return None for non-identifiers
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_prepare_rename_at_whitespace() {
        let source = "class Foo {  }";
        let (provider, file_id, uri) = create_test_provider(source);

        // Position in whitespace
        let position = Position::new(0, 11);
        let result = provider.prepare_rename(file_id, position, source, &uri);

        assert!(result.is_ok(), "Whitespace rename check should not error");
        assert!(result.unwrap().is_none());
    }

    fn java_token_at(source: &str, byte_offset: usize) -> Result<Option<RenameToken>, String> {
        let file_id = FileId::new(7_001);
        let mut db = Database::default();
        db.set_file_text_tracked(file_id, Arc::new(source.to_string()));
        db.set_file_language(file_id, Language::Java);
        let position = LineIndex::new(source).offset_to_position(source, byte_offset);
        java_rename_token(&db.snapshot(), file_id, position, source)
    }

    #[test]
    fn contextual_words_are_renameable_only_in_identifier_positions() {
        let source = "class C { int module; void yield() {} void m(Object value) { var var = value; module++; this.yield(); } }";
        for occurrence in [
            source.find("module;").expect("field name"),
            source.find("yield() {}").expect("method name"),
            source.find("var = value").expect("inferred local name"),
        ] {
            assert!(
                java_token_at(source, occurrence)
                    .expect("token validation")
                    .is_some(),
                "contextual word at {occurrence} must be an identifier"
            );
        }

        let inferred_type = source.find("var var").expect("inferred type");
        assert!(
            java_token_at(source, inferred_type).is_err(),
            "the first var is a restricted type token"
        );

        let yield_source = "class C { int m(int value) { return switch(value) { default -> { yield value; } }; } }";
        let yield_keyword = yield_source.find("yield value").expect("yield statement");
        assert!(java_token_at(yield_source, yield_keyword).is_err());
    }

    #[test]
    fn local_contextual_identifier_rename_does_not_edit_the_var_type_token() {
        let source =
            "class C { void m(Object value) { var var = value; System.out.println(var); } }";
        let (_temp, provider, file_id, uri) =
            create_isolated_test_provider(source, "java", Language::Java);
        let declaration = source.find("var = value").expect("local name");
        let position = LineIndex::new(source).offset_to_position(source, declaration);

        let result = provider
            .rename(file_id, position, "record", source, &uri)
            .expect("rename")
            .expect("contextual local rename available");
        let edits = result
            .edit
            .changes
            .expect("plain edit")
            .remove(&uri)
            .expect("source edits");
        assert_eq!(edits.len(), 2, "declaration and lexical reference only");
        assert!(edits.iter().all(|edit| edit.new_text == "record"));
        assert!(edits.iter().all(|edit| {
            let start = position_to_offset(source, edit.range.start).expect("edit start") as usize;
            start != source.find("var var").expect("inferred type")
        }));
    }

    #[test]
    fn test_is_valid_java_identifier_valid() {
        assert!(is_valid_java_identifier("myVar", None));
        assert!(is_valid_java_identifier("MyClass", None));
        assert!(is_valid_java_identifier("_private", None));
        assert!(is_valid_java_identifier("$special", None));
        assert!(is_valid_java_identifier("camelCase123", None));
        assert!(is_valid_java_identifier("CONSTANT_NAME", None));
        assert!(is_valid_java_identifier("a", None));
        // Note: "_" alone is a reserved keyword in Java 9+
        assert!(is_valid_java_identifier("__", None)); // Double underscore is valid
        assert!(is_valid_java_identifier("$", None));
        assert!(is_valid_java_identifier(
            "module",
            Some(LspSymbolKind::FIELD)
        ));
        assert!(is_valid_java_identifier(
            "record",
            Some(LspSymbolKind::FIELD)
        ));
        assert!(is_valid_java_identifier(
            "yield",
            Some(LspSymbolKind::FIELD)
        ));
        assert!(is_valid_java_identifier("\u{20ac}value", None));
        assert!(is_valid_java_identifier("\u{037a}value", None));
        assert!(is_valid_java_identifier("e\u{301}", None));
        assert!(is_valid_java_identifier(
            "module",
            Some(LspSymbolKind::PACKAGE)
        ));
        assert!(is_valid_java_identifier("when", Some(LspSymbolKind::CLASS)));
        assert!(is_valid_identifier(
            Language::Kotlin,
            Some(LspSymbolKind::FIELD),
            "get"
        ));
    }

    #[test]
    fn test_is_valid_java_identifier_invalid() {
        // Empty
        assert!(!is_valid_java_identifier("", None));

        // Starts with digit
        assert!(!is_valid_java_identifier("123abc", None));
        assert!(!is_valid_java_identifier("1", None));

        // Contains invalid characters
        assert!(!is_valid_java_identifier("my-var", None));
        assert!(!is_valid_java_identifier("my var", None));
        assert!(!is_valid_java_identifier("my.var", None));
        assert!(!is_valid_java_identifier("my@var", None));
        // Unicode XID and Java identifiers intentionally diverge. U+2118 is
        // Other_ID_Start and U+00B7 is Other_ID_Continue, but neither has a
        // Java identifier category.
        assert!(!is_valid_java_identifier("\u{2118}value", None));
        assert!(!is_valid_java_identifier("value\u{00b7}", None));
        assert!(!is_valid_java_identifier("f\u{200c}oo", None));
        assert!(!is_valid_java_identifier("cl\u{200c}ass", None));

        // Reserved keywords
        for keyword in [
            "class", "int", "void", "if", "while", "true", "false", "null", "this", "super",
        ] {
            assert!(!is_valid_java_identifier(keyword, None), "{keyword}");
        }

        for restricted_type_name in ["sealed", "permits", "var", "yield", "record"] {
            assert!(!is_valid_java_identifier(
                restricted_type_name,
                Some(LspSymbolKind::CLASS)
            ));
            assert!(!is_valid_java_identifier(
                restricted_type_name,
                Some(LspSymbolKind::TYPE_PARAMETER)
            ));
        }
        assert!(!is_valid_java_identifier(
            "yield",
            Some(LspSymbolKind::METHOD)
        ));
    }

    #[test]
    fn test_is_java_identifier_start() {
        // Valid starts
        assert!(is_java_identifier_start('a'));
        assert!(is_java_identifier_start('Z'));
        assert!(is_java_identifier_start('_'));
        assert!(is_java_identifier_start('$'));
        assert!(is_java_identifier_start('ä')); // Unicode letter

        // Invalid starts
        assert!(!is_java_identifier_start('0'));
        assert!(!is_java_identifier_start('9'));
        assert!(!is_java_identifier_start('-'));
        assert!(!is_java_identifier_start(' '));
    }

    #[test]
    fn test_is_java_identifier_part() {
        // Valid parts
        assert!(is_java_identifier_part('a'));
        assert!(is_java_identifier_part('Z'));
        assert!(is_java_identifier_part('_'));
        assert!(is_java_identifier_part('$'));
        assert!(is_java_identifier_part('0'));
        assert!(is_java_identifier_part('9'));
        assert!(is_java_identifier_part('\u{200c}'));

        // Invalid parts
        assert!(!is_java_identifier_part('-'));
        assert!(!is_java_identifier_part(' '));
        assert!(!is_java_identifier_part('.'));
    }
}
