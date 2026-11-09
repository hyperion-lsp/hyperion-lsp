// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Public revision, query, and publication types for the workspace index.

use super::{
    ExternalSourceArchive, ExternalSourceIdentity, ExternalSourcePublicationGate,
    ExternalSourceRefreshAuthority, RegisteredFilePath, PROFILE_LANGUAGE_COUNT,
};
use crate::{FileId, Symbol, SymbolLocation};
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::path::Path;
use std::sync::{atomic::Ordering, Arc};
use tracing::info;

pub type ExternalIndexDataBySource = (
    ExternalSourceIdentity,
    Vec<crate::Symbol>,
    Vec<crate::TypeGenericSupertypes>,
);

#[derive(Clone, Copy, Debug, Default)]
pub struct IndexPhaseSnapshot {
    pub files: [u64; PROFILE_LANGUAGE_COUNT],
    pub read_ns: u64,
    pub hash_ns: u64,
    pub setup_lock_ns: u64,
    pub symbol_extract_ns: [u64; PROFILE_LANGUAGE_COUNT],
    pub usage_extract_ns: [u64; PROFILE_LANGUAGE_COUNT],
    pub parse_cache_ns: u64,
    pub store_lock_ns: u64,
    pub post_process_ns: u64,
}

impl IndexPhaseSnapshot {
    fn millis(ns: u64) -> f64 {
        ns as f64 / 1_000_000.0
    }

    pub fn log(&self, label: &str) {
        const NAMES: [&str; PROFILE_LANGUAGE_COUNT] = ["java", "kotlin", "groovy", "scala"];
        info!(
            target: "hyperion_workspace::index_profile",
            "{label}: aggregate worker time: read={:.1}ms hash={:.1}ms setup_lock={:.1}ms parse_cache={:.1}ms store_lock={:.1}ms post={:.1}ms",
            Self::millis(self.read_ns),
            Self::millis(self.hash_ns),
            Self::millis(self.setup_lock_ns),
            Self::millis(self.parse_cache_ns),
            Self::millis(self.store_lock_ns),
            Self::millis(self.post_process_ns),
        );
        for (index, name) in NAMES.iter().enumerate() {
            info!(
                target: "hyperion_workspace::index_profile",
                "{label}: {name}: files={} symbol_extract={:.1}ms usage_extract={:.1}ms",
                self.files[index],
                Self::millis(self.symbol_extract_ns[index]),
                Self::millis(self.usage_extract_ns[index]),
            );
        }
    }
}

/// Immutable per-file method-name ranges paired with their exact declaration FQNs.
pub type ResolvedMethodUsages = Arc<[(crate::TextRange, Arc<str>)]>;

/// Identity of one exact source publication.
///
/// Content hashes identify bytes, not publications: publishing identical text
/// again must still retire every range captured from the prior workspace state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileRevisionToken {
    pub(super) file_id: FileId,
    pub(super) revision_id: NonZeroU64,
}

/// One exact disk publication seeded from the persistent workspace cache.
///
/// The registered path retains the identity captured at restoration time. A
/// later manifest reconciliation must compare this key, not re-resolve the
/// display spelling after a symlink or filesystem alias changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredFileRevision {
    pub registered_path: RegisteredFilePath,
    pub revision: FileRevisionToken,
}

/// Exact generation of workspace-wide name and hierarchy resolution state.
///
/// This is distinct from a file token: publishing another file can rebind an
/// unchanged `extends Base` edge through package or import resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceRevision(pub(super) NonZeroU64);

/// Authority to publish one extraction of an external archive or virtual
/// source.
///
/// External `FileId`s deliberately remain stable while a classpath retains a
/// path. The generation distinguishes overlapping refreshes of that same path:
/// only the most recently reserved token may publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExternalSourceRevisionToken {
    pub(super) file_id: FileId,
    pub(super) generation: NonZeroU64,
}

impl ExternalSourceRevisionToken {
    pub fn file_id(self) -> FileId {
        self.file_id
    }

    pub fn generation(self) -> NonZeroU64 {
        self.generation
    }
}

/// Result of atomically replacing one logical classpath.
///
/// Callers must retain the revision corresponding to each archive and pass it
/// to the revisioned indexing API after extraction. This prevents a cancelled
/// older refresh from acquiring authority after a newer refresh has started.
#[derive(Debug, Clone, Default)]
#[must_use = "archive indexing must use the revisions reserved by this refresh"]
pub struct ExternalSourceRefresh {
    pub(super) revisions: HashMap<ExternalSourceIdentity, ExternalSourceRevisionToken>,
    pub(super) released_file_ids: Vec<FileId>,
}

/// Pure archive-extraction result awaiting one short WorkspaceIndex commit.
///
/// Parsing and Javadoc merging happen before this value is created. Publishing
/// it performs only revision validation and index mutation, which lets callers
/// contain parser panics without accidentally treating a partial index mutation
/// as an ordinary per-archive failure.
#[must_use = "a prepared archive must be published or deliberately discarded"]
pub struct PreparedExternalArchivePublication {
    pub(super) archive: ExternalSourceArchive,
    pub(super) revision: ExternalSourceRevisionToken,
    pub(super) symbols: Vec<Symbol>,
    pub(super) generic_supertypes: Vec<crate::TypeGenericSupertypes>,
    pub(super) stats: crate::JarIndexStats,
}

impl PreparedExternalArchivePublication {
    /// Record failure of an optional documentation attachment while retaining
    /// the successfully prepared binary symbols.
    pub fn set_documentation_error(&mut self, error: String) {
        self.stats.documentation_loaded = false;
        self.stats.documentation_error = Some(error);
    }
}

#[derive(Debug, Clone)]
pub struct ExternalSourceRefreshTicket {
    pub(super) authority: Arc<ExternalSourceRefreshAuthority>,
    pub(super) generation: NonZeroU64,
}

/// Short-lived linearization guard for one external classpath owner.
///
/// Dependency resolution happens before this guard is acquired. Callers hold
/// it only while publishing the workspace membership and companion side
/// registries. Starting the next refresh for the same owner waits for that
/// small commit section, so it cannot invalidate an otherwise coherent
/// generation halfway through publication.
pub struct ExternalSourcePublicationGuard {
    gate: Arc<ExternalSourcePublicationGate>,
}

impl ExternalSourcePublicationGuard {
    pub(super) fn acquire(gate: Arc<ExternalSourcePublicationGate>) -> Self {
        {
            let mut locked = gate.locked.lock();
            while *locked {
                gate.unlocked.wait(&mut locked);
            }
            *locked = true;
        }
        Self { gate }
    }
}

impl Drop for ExternalSourcePublicationGuard {
    fn drop(&mut self) {
        let mut locked = self.gate.locked.lock();
        debug_assert!(*locked, "an acquired publication gate must be locked");
        *locked = false;
        self.gate.unlocked.notify_one();
    }
}

impl ExternalSourceRefreshTicket {
    /// Logical classpath owner whose refresh this ticket may publish.
    pub fn owner(&self) -> &str {
        &self.authority.owner
    }

    /// Monotonic generation shared with companion side-registry publication.
    pub fn generation(&self) -> NonZeroU64 {
        self.generation
    }

    /// Whether no newer attempt has been reserved by this exact index owner.
    pub fn is_latest(&self) -> bool {
        self.authority.latest_generation.load(Ordering::Acquire) == self.generation.get()
    }

    /// Whether this exact classpath generation is request-visible.
    pub fn is_committed(&self) -> bool {
        self.authority.committed_generation.load(Ordering::Acquire) == self.generation.get()
    }

    /// Whether both tickets were minted by the same WorkspaceIndex owner.
    pub fn has_same_authority(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.authority, &other.authority)
    }
}

impl PartialEq for ExternalSourceRefreshTicket {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation && self.has_same_authority(other)
    }
}

impl Eq for ExternalSourceRefreshTicket {}

impl ExternalSourceRefresh {
    /// Return the publication revision reserved for `source` by this refresh.
    pub fn revision_for(&self, source: &Path) -> Option<ExternalSourceRevisionToken> {
        self.revisions
            .get(&ExternalSourceIdentity::for_legacy_path(source))
            .copied()
    }

    /// Return the publication revision for an identity captured before the
    /// refresh began. Unlike [`Self::revision_for`], this does not observe the
    /// filesystem again and is therefore safe across symlink retargeting.
    pub fn revision_for_identity(
        &self,
        source: &ExternalSourceIdentity,
    ) -> Option<ExternalSourceRevisionToken> {
        self.revisions.get(source).copied()
    }

    /// External identities that ceased to be owned by any classpath.
    pub fn released_file_ids(&self) -> &[FileId] {
        &self.released_file_ids
    }
}

impl WorkspaceRevision {
    pub fn get(self) -> NonZeroU64 {
        self.0
    }
}

impl FileRevisionToken {
    pub fn file_id(self) -> FileId {
        self.file_id
    }

    pub fn revision_id(self) -> NonZeroU64 {
        self.revision_id
    }
}

/// One location paired with the exact file revision that owns its byte range.
#[derive(Debug, Clone)]
pub struct RevisionedLocation {
    pub revision: FileRevisionToken,
    pub location: SymbolLocation,
}

/// Source authority that owns a typed definition's byte range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionSourceAuthority {
    /// Workspace/editor source whose range belongs to this exact publication.
    Workspace(FileRevisionToken),
    /// Dependency/JDK declaration owned by the surrounding workspace revision.
    External,
}

/// One semantic declaration selected atomically with its metadata and source.
///
/// `workspace_revision` proves which source-precedence graph elected the
/// declaration. Workspace byte ranges additionally carry a file publication;
/// external ranges are valid only while that workspace/classpath revision is
/// current.
#[derive(Debug, Clone)]
pub struct ResolvedDefinition {
    pub target: crate::SymbolTarget,
    pub symbol: Arc<crate::Symbol>,
    pub location: SymbolLocation,
    pub workspace_revision: WorkspaceRevision,
    pub source: DefinitionSourceAuthority,
}

/// Source spelling used to present a variadic parameter. Applicability always
/// consumes the normalized `type_name`; this enum is presentation metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectedVariadicStyle {
    None,
    JavaEllipsis,
    KotlinPrefix,
}

/// One receiver-relative callable parameter captured with its source display
/// metadata. `type_name` is normalized for semantic parsing; `name` and
/// `variadic_style` preserve the declaration's user-facing shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedCallableParameter {
    pub type_name: String,
    pub name: Option<Arc<str>>,
    pub variadic_style: ProjectedVariadicStyle,
}

/// One declaring-owner type parameter expressed in the queried receiver.
/// Raw arguments are represented by their qualified leftmost-bound erasure,
/// so semantic consumers do not need a second index read to reconstruct them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedOwnerBinding {
    pub name: Arc<str>,
    pub type_name: String,
}

impl ProjectedCallableParameter {
    #[must_use]
    pub fn declaration(&self) -> String {
        self.declaration_with_type(&self.type_name)
    }

    #[must_use]
    pub fn declaration_with_type(&self, projected_type: &str) -> String {
        let mut type_name = projected_type.to_owned();
        if self.variadic_style == ProjectedVariadicStyle::JavaEllipsis {
            type_name = type_name.strip_suffix("[]").map_or_else(
                || format!("{type_name}..."),
                |element| format!("{element}..."),
            );
        } else if self.variadic_style == ProjectedVariadicStyle::KotlinPrefix {
            type_name = type_name
                .strip_suffix("[]")
                .unwrap_or(type_name.as_str())
                .to_owned();
        }
        let declaration = self
            .name
            .as_deref()
            .map_or_else(|| type_name.clone(), |name| format!("{type_name} {name}"));
        if self.variadic_style == ProjectedVariadicStyle::KotlinPrefix {
            format!("vararg {declaration}")
        } else {
            declaration
        }
    }

    #[must_use]
    pub fn is_variadic(&self) -> bool {
        self.variadic_style != ProjectedVariadicStyle::None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedCallableSignature {
    pub parameters: Vec<ProjectedCallableParameter>,
    pub return_type: Option<String>,
    /// Compact owner substitutions captured under the same index read as the
    /// declaration, parameter, and return projections.
    pub owner_bindings: Vec<ProjectedOwnerBinding>,
    /// Method-level parameters with owner variables in their bounds projected
    /// through the receiver. The method variables themselves remain intact.
    pub type_parameters: Vec<crate::TypeParameterInfo>,
}

impl ProjectedCallableSignature {
    /// Render receiver-relative types using the declaration's source parameter
    /// names and variadic spelling. Applicability consumes `parameters[*].type_name`.
    #[must_use]
    pub fn parameter_declarations(&self) -> Vec<String> {
        self.parameters
            .iter()
            .map(ProjectedCallableParameter::declaration)
            .collect()
    }
}

/// One effective method and its receiver-relative generic projection.
///
/// `projection == None` is an explicit conservative result: the declaration
/// exists, but a malformed or conflicting generic hierarchy prevents a
/// trustworthy signature. Consumers must not substitute the raw declaration
/// spelling in that case.
#[derive(Debug, Clone)]
pub struct ProjectedEffectiveMethod {
    pub definition: ResolvedDefinition,
    pub projection: Option<ProjectedCallableSignature>,
}

/// One method a receiver inherits and could legally redeclare.
///
/// Produced by [`crate::index::WorkspaceIndex::find_inherited_methods`], which is the single
/// hierarchy walk every "override"/"implement" editor feature must use.
/// Declarations that cannot be redeclared at all -- constructors,
/// `static`, `final` and `private` -- are already excluded, as is any slot the
/// receiver itself won, so a caller only has to decide presentation.
#[derive(Debug, Clone)]
pub struct InheritedMethod {
    pub definition: ResolvedDefinition,
    /// FQN of the type that declares the elected method.
    pub declaring_owner: String,
    /// `true` when the declaration is abstract, so a concrete receiver must
    /// supply a body rather than merely being permitted to.
    pub requires_implementation: bool,
    /// `true` when the declaring owner is an interface. A redeclaration that
    /// delegates to the inherited body must then spell it
    /// `Owner.super.member(...)`: plain `super` denotes the superclass
    /// (JLS 15.12.1), so it cannot reach an interface `default` method.
    pub declaring_owner_is_interface: bool,
    /// Receiver-relative signature, carrying the same fail-closed meaning as
    /// on [`ProjectedEffectiveMethod`]: `None` means the generic hierarchy
    /// could not be projected, not that the declaration is absent.
    pub projection: Option<ProjectedCallableSignature>,
}

/// One unambiguous effective field and its receiver-relative type.
///
/// As with [`ProjectedEffectiveMethod`], a missing projection is different
/// from a missing declaration and must remain a fail-closed answer. It covers
/// both absent field-type metadata and an invalid/conflicting generic path;
/// neither is sufficient evidence for a semantic type.
#[derive(Debug, Clone)]
pub struct ProjectedEffectiveField {
    pub definition: ResolvedDefinition,
    pub projected_type: Option<String>,
}

/// How a Java field selector is qualified at its use site.
///
/// Protected access outside the declaring package has a stricter receiver
/// rule for ordinary expressions than it does for `super` access, so callers
/// must preserve this distinction instead of applying visibility after member
/// election with incomplete context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldAccessQualifier {
    Expression,
    Super,
}

/// Canonical result of field election, generic projection, and Java access
/// checking under one coherent index read.
#[derive(Debug, Clone)]
pub enum ProjectedFieldAccessResolution {
    Resolved(ProjectedEffectiveField),
    Inaccessible,
    Ambiguous,
    Missing,
}

impl ResolvedDefinition {
    pub fn revision(&self) -> Option<FileRevisionToken> {
        match self.source {
            DefinitionSourceAuthority::Workspace(revision) => Some(revision),
            DefinitionSourceAuthority::External => None,
        }
    }

    pub fn revisioned_location(&self) -> Option<RevisionedLocation> {
        self.revision().map(|revision| RevisionedLocation {
            revision,
            location: self.location.clone(),
        })
    }
}

/// Locations from one file, sharing one revision token.
#[derive(Debug, Clone)]
pub struct RevisionedFileLocations {
    pub revision: FileRevisionToken,
    pub locations: Vec<SymbolLocation>,
}

/// A multi-file range result captured from one coherent index read.
/// Bytecode-only locations without a published source revision are omitted.
#[derive(Debug, Clone, Default)]
pub struct RevisionedLocations {
    pub files: Vec<RevisionedFileLocations>,
}

/// Complete usage result resolved from exact current workspace sources.
///
/// `scanned_revisions` includes files with no matching usages. Retaining those
/// negative inputs is essential: revalidating only files that produced a hit
/// could miss a reference introduced by a concurrent publication.
#[derive(Debug, Clone)]
pub struct CurrentUsageLocations {
    pub workspace_revision: WorkspaceRevision,
    pub files: Vec<RevisionedFileLocations>,
    pub(super) scanned_revisions: Arc<[FileRevisionToken]>,
    pub(super) resource_proof: Option<Arc<crate::RefactoringResourceProof>>,
}

impl CurrentUsageLocations {
    pub fn scanned_revisions(&self) -> &[FileRevisionToken] {
        &self.scanned_revisions
    }

    pub fn locations(&self) -> impl Iterator<Item = &SymbolLocation> {
        self.files.iter().flat_map(|file| file.locations.iter())
    }

    pub fn resource_proof(&self) -> Option<&Arc<crate::RefactoringResourceProof>> {
        self.resource_proof.as_ref()
    }
}

/// Why an exact current usage query could not make a complete claim.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum CurrentUsageQueryError {
    #[error("current usage query was cancelled")]
    Cancelled,
    #[error("current usage query is unavailable: {0}")]
    Unavailable(String),
    #[error("workspace kept changing while current usages were resolved")]
    ConcurrentModification,
}

/// Declaration dependencies re-resolved from one exact current source.
#[derive(Debug, Clone)]
pub struct CurrentFileDependencies {
    pub workspace_revision: WorkspaceRevision,
    pub revision: FileRevisionToken,
    pub fqns: Vec<Arc<str>>,
}

/// Resolved method-name ranges with both their exact source publication and
/// the workspace context that selected each declaration FQN.
#[derive(Debug, Clone)]
pub struct RevisionedResolvedMethodUsages {
    pub revision: FileRevisionToken,
    pub workspace_revision: WorkspaceRevision,
    pub usages: ResolvedMethodUsages,
}

/// Provenance of the source buffer that produced one published index revision.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRevisionOrigin {
    /// An editor buffer or another caller-provided in-memory source.
    EditorOrCaller,
    /// A source revision read from the workspace filesystem.
    Disk,
}

/// Statistics about the index state
pub struct IndexStats {
    pub files_scanned: usize,
    pub files_indexed: usize,
    pub files_failed: usize,
    pub symbols_indexed: usize,
}

/// File indexing statistics
#[derive(Debug, Clone, Default)]
pub struct FileIndexStats {
    pub symbols_added: usize,
    /// Declaration identities whose old or new shape may affect dependents.
    /// Includes removed FQNs, unlike querying the file after publication.
    pub affected_fqns: Vec<Arc<str>>,
    /// Files depending on either removed or newly published declarations,
    /// captured before historical reverse edges are discarded.
    pub dependent_files: Vec<FileId>,
    /// File identities displaced from the destination by this publication.
    /// Callers that mirror source state outside the workspace index must retire
    /// these identities from those layers as part of the same transaction.
    pub retired_file_ids: Vec<FileId>,
}

/// Result of attempting to retire one exact path/FileId revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileRevisionRemovalOutcome {
    /// The path no longer names the supplied `FileId`; no callbacks ran.
    RevisionChanged,
    /// The caller inspected the locked revision and chose to keep it.
    Retained,
    /// The exact revision and all index-owned state were removed.
    Removed { dependent_files: Vec<FileId> },
}
