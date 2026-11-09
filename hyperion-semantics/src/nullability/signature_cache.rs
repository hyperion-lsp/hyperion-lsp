// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method Signature Cache for Cross-File Nullability Invalidation
//!
//! This module provides a workspace-wide cache of method signatures with their
//! nullability annotations. When a method's annotations change in file A, we can
//! invalidate analysis of files that call those methods.
//!
//! ## Design
//!
//! - **FQN Format:** `package.ClassName.methodName(ParamType1,ParamType2)`
//!   - Includes parameter types to handle overloaded methods
//!   - Example: `com.example.UserService.findUser(String,int)`
//!
//! - **Version Tracking:** immutable, structurally shared snapshots are
//!   published as Salsa inputs after signature changes
//!   - Requests see the exact contract revision paired with their snapshot
//!   - Version comparison remains O(1)
//!
//! - **Publication:** one `RwLock` serializes writers of an immutable snapshot
//!   whose persistent maps are structurally shared by request snapshots
//!   - Readers clone roots in O(1) instead of copying workspace-sized maps
//!   - A file revision is assembled off to the side and installed in one write

use super::analysis::InvocationEffect;
use super::patterns::ValueKind;
use super::{
    extract_method_info, has_nonnull_annotation, has_nullable_annotation, MethodCallKey,
    Nullability,
};
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_types::field_declarators;
use hyperion_workspace::{FileId, FileRevisionToken, WorkspaceRevision};
use im::{HashMap as PersistentHashMap, HashSet as PersistentHashSet};
use parking_lot::RwLock;
use rowan::NodeOrToken;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Complete signature of a method including nullability information
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MethodSignature {
    /// Nullability of the return type
    pub return_nullability: Nullability,
    /// Nullability of each parameter (in order)
    pub param_nullabilities: Vec<Nullability>,
    /// File where this method is defined (for invalidation)
    pub file_id: FileId,
    /// Optional semantic facts are private so adding another conservative fact
    /// does not make every external constructor a source-breaking struct
    /// literal. Use the builders/accessors below.
    is_static: bool,
    return_value_kind: ValueKind,
    invocation_effect: InvocationEffect,
}

/// Compact declaration facts needed when a field use crosses a file boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub struct FieldSignature {
    value_kind: ValueKind,
    flags: u8,
    package_segments: u8,
}

impl FieldSignature {
    const STATIC: u8 = 1 << 0;
    const VISIBILITY_SHIFT: u8 = 1;
    const VISIBILITY_MASK: u8 = 0b11 << Self::VISIBILITY_SHIFT;
    const NULLABILITY_SHIFT: u8 = 3;
    const NULLABILITY_MASK: u8 = 0b111 << Self::NULLABILITY_SHIFT;

    #[cfg(test)]
    const fn new(value_kind: ValueKind, is_static: bool, visibility: FieldVisibility) -> Self {
        Self::from_source(value_kind, is_static, visibility, Nullability::Unknown, 0)
    }

    const fn from_source(
        value_kind: ValueKind,
        is_static: bool,
        visibility: FieldVisibility,
        nullability: Nullability,
        package_segments: u8,
    ) -> Self {
        let visibility = match visibility {
            FieldVisibility::Private => 0,
            FieldVisibility::Package => 1,
            FieldVisibility::Protected => 2,
            FieldVisibility::Public => 3,
        } << Self::VISIBILITY_SHIFT;
        let nullability = match nullability {
            Nullability::NonNull => 0,
            Nullability::Null => 1,
            Nullability::Nullable => 2,
            Nullability::Unknown => 3,
            Nullability::Bottom => 4,
        } << Self::NULLABILITY_SHIFT;
        Self {
            value_kind,
            flags: (if is_static { Self::STATIC } else { 0 }) | visibility | nullability,
            package_segments,
        }
    }

    pub(crate) const fn value_kind(self) -> ValueKind {
        self.value_kind
    }

    pub(crate) const fn is_static(self) -> bool {
        self.flags & Self::STATIC != 0
    }

    pub(crate) const fn nullability(self) -> Nullability {
        match (self.flags & Self::NULLABILITY_MASK) >> Self::NULLABILITY_SHIFT {
            0 => Nullability::NonNull,
            1 => Nullability::Null,
            2 => Nullability::Nullable,
            4 => Nullability::Bottom,
            _ => Nullability::Unknown,
        }
    }

    const fn visibility(self) -> FieldVisibility {
        match (self.flags & Self::VISIBILITY_MASK) >> Self::VISIBILITY_SHIFT {
            0 => FieldVisibility::Private,
            1 => FieldVisibility::Package,
            2 => FieldVisibility::Protected,
            _ => FieldVisibility::Public,
        }
    }

    /// Return the declaring package from the canonical owner retained by the
    /// declaration map. `package_segments` is only a delimiter into that
    /// owner; it is never interpreted as package identity on its own.
    fn declaring_package_matches(self, owner: &str, package: &str) -> bool {
        let expected_segments = usize::from(self.package_segments);
        expected_segments == package.split('.').count()
            && owner
                .split('.')
                .take(expected_segments)
                .eq(package.split('.'))
    }

    fn declaring_package_matches_context(self, owner: &str, package: Option<&str>) -> bool {
        package.map_or(self.package_segments == 0, |package| {
            self.declaring_package_matches(owner, package)
        })
    }

    #[cfg(test)]
    const fn is_public(self) -> bool {
        matches!(self.visibility(), FieldVisibility::Public)
    }
}

fn exact_owner_packages_match(
    left_owner: &str,
    left_package_segments: u8,
    right_owner: &str,
    right_package_segments: u8,
) -> bool {
    left_package_segments != u8::MAX
        && left_package_segments == right_package_segments
        && left_owner
            .split('.')
            .take(usize::from(left_package_segments))
            .eq(right_owner
                .split('.')
                .take(usize::from(right_package_segments)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldVisibility {
    Private,
    Package,
    Protected,
    Public,
}

impl MethodSignature {
    #[must_use]
    pub const fn new(
        return_nullability: Nullability,
        param_nullabilities: Vec<Nullability>,
        file_id: FileId,
    ) -> Self {
        Self {
            return_nullability,
            param_nullabilities,
            file_id,
            is_static: false,
            return_value_kind: ValueKind::Unknown,
            invocation_effect: InvocationEffect::Arbitrary,
        }
    }

    #[must_use]
    pub const fn with_static(mut self, is_static: bool) -> Self {
        self.is_static = is_static;
        self
    }

    #[must_use]
    pub const fn is_static(&self) -> bool {
        self.is_static
    }

    pub(crate) const fn with_return_value_kind(mut self, kind: ValueKind) -> Self {
        self.return_value_kind = kind;
        self
    }

    pub(crate) const fn return_value_kind(&self) -> ValueKind {
        self.return_value_kind
    }

    #[cfg(test)]
    pub(crate) const fn with_invocation_may_throw(mut self, may_throw: bool) -> Self {
        self.invocation_effect = if may_throw {
            InvocationEffect::Arbitrary
        } else {
            InvocationEffect::NonThrowing
        };
        self
    }

    #[cfg(test)]
    pub(crate) const fn invocation_may_throw(&self) -> bool {
        !matches!(self.invocation_effect, InvocationEffect::NonThrowing)
    }

    pub(crate) const fn with_invocation_effect(mut self, effect: InvocationEffect) -> Self {
        self.invocation_effect = effect;
        self
    }

    pub(crate) const fn invocation_effect(&self) -> InvocationEffect {
        self.invocation_effect
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MethodContractSummary {
    pub(crate) return_nullability: Nullability,
    pub(crate) param_nullabilities: Vec<Nullability>,
}

#[derive(Debug)]
pub(crate) struct MethodContract<'a> {
    pub(crate) return_nullability: Nullability,
    pub(crate) param_nullabilities: Cow<'a, [Nullability]>,
}

/// Immutable, structurally shared view of all published method signatures.
///
/// Salsa stores this value as an input. A request snapshot therefore observes
/// exactly the signature revision paired with its Salsa revision, even while a
/// later editor update publishes a replacement into the live database.
#[derive(Debug, Clone)]
pub struct MethodSignatureSnapshot {
    version: u64,
    signatures: PersistentHashMap<Arc<str>, PersistentHashMap<FileId, MethodSignature>>,
    file_signatures: PersistentHashMap<FileId, PersistentHashSet<Arc<str>>>,
    declaration_facts: Arc<DeclarationFacts>,
    known_files: PersistentHashMap<FileId, PublishedFileRevision>,
    indexed_call_bindings: PersistentHashMap<FileId, IndexedCallBindings>,
    call_variants: PersistentHashMap<MethodCallKey, PersistentHashSet<(Arc<str>, FileId)>>,
    call_summaries: PersistentHashMap<MethodCallKey, MethodContractSummary>,
    vararg_variants: PersistentHashMap<Arc<str>, VarargsByMethod>,
    declaration_aliases: PersistentHashMap<Arc<str>, PersistentHashSet<Arc<str>>>,
}

#[derive(Debug, Clone, Default)]
struct DeclarationFacts {
    /// Sorted field definitions grouped by owner. Most fields have one source
    /// definition, so a compact shared slice avoids allocating two persistent
    /// HAMT roots (name and FileId) for every singleton field.
    fields: PersistentHashMap<Arc<str>, Arc<[FieldDefinition]>>,
    file_fields: PersistentHashMap<FileId, PersistentHashSet<FieldDeclaration>>,
    declared_type_counts: PersistentHashMap<Arc<str>, u32>,
    file_declared_types: PersistentHashMap<FileId, PersistentHashSet<Arc<str>>>,
    /// Duplicate owner declarations are uncommon. A sorted shared slice keeps
    /// the common one-definition case to one allocation instead of allocating
    /// a nested persistent-map root for every source type.
    type_hierarchies: PersistentHashMap<Arc<str>, Arc<[HierarchyDefinition]>>,
    file_hierarchy_owners: PersistentHashMap<FileId, PersistentHashSet<Arc<str>>>,
}

type FieldReplacements = HashMap<FieldDeclaration, FieldSignature>;
type HierarchyReplacements = HashMap<Arc<str>, DirectHierarchy>;

fn declared_owner_package_segments(
    owner: &str,
    declared_types: &PersistentHashSet<Arc<str>>,
) -> Option<u8> {
    let top_level = declared_types
        .iter()
        .filter(|candidate| {
            owner == candidate.as_ref()
                || owner
                    .strip_prefix(candidate.as_ref())
                    .is_some_and(|suffix| suffix.starts_with('.'))
        })
        .min_by_key(|candidate| candidate.split('.').count())?;
    top_level.split('.').count().checked_sub(1)?.try_into().ok()
}

impl DeclarationFacts {
    fn declared_types_match(
        &self,
        file_id: FileId,
        replacements: &PersistentHashSet<Arc<str>>,
    ) -> bool {
        self.file_declared_types
            .get(&file_id)
            .cloned()
            .unwrap_or_default()
            == *replacements
    }

    fn replace_declared_types(
        &mut self,
        file_id: FileId,
        replacements: PersistentHashSet<Arc<str>>,
    ) {
        let old_types = self
            .file_declared_types
            .get(&file_id)
            .cloned()
            .unwrap_or_default();
        for declared_type in old_types
            .iter()
            .filter(|declared_type| !replacements.contains(*declared_type))
        {
            let count = self
                .declared_type_counts
                .get(declared_type)
                .copied()
                .expect("file type must have a matching reverse count");
            if count == 1 {
                self.declared_type_counts.remove(declared_type);
            } else {
                self.declared_type_counts
                    .insert(Arc::clone(declared_type), count - 1);
            }
        }
        for declared_type in replacements
            .iter()
            .filter(|declared_type| !old_types.contains(*declared_type))
        {
            let count = self
                .declared_type_counts
                .get(declared_type)
                .copied()
                .unwrap_or_default();
            self.declared_type_counts.insert(
                Arc::clone(declared_type),
                count
                    .checked_add(1)
                    .expect("declared type occurrence count overflowed"),
            );
        }
        if replacements.is_empty() {
            self.file_declared_types.remove(&file_id);
        } else {
            self.file_declared_types.insert(file_id, replacements);
        }
    }

    fn fields_match(&self, file_id: FileId, replacements: &FieldReplacements) -> bool {
        let old_keys = self.file_fields.get(&file_id).cloned().unwrap_or_default();
        old_keys.len() == replacements.len()
            && old_keys.iter().all(|declaration| {
                replacements.get(declaration).is_some_and(|replacement| {
                    self.fields
                        .get(&declaration.owner)
                        .and_then(|fields| {
                            field_definitions_named(fields, &declaration.name)
                                .find(|definition| definition.file_id == file_id)
                        })
                        .map(|definition| definition.signature)
                        == Some(*replacement)
                })
            })
    }

    fn replace_fields(&mut self, file_id: FileId, replacements: FieldReplacements) {
        let old_keys = self.file_fields.get(&file_id).cloned().unwrap_or_default();
        let mut affected_owners: std::collections::HashSet<Arc<str>> = old_keys
            .iter()
            .map(|declaration| Arc::clone(&declaration.owner))
            .collect();
        affected_owners.extend(
            replacements
                .keys()
                .map(|declaration| Arc::clone(&declaration.owner)),
        );
        let replacement_keys: PersistentHashSet<_> = replacements.keys().cloned().collect();
        for owner in affected_owners {
            let mut definitions: Vec<_> = self
                .fields
                .get(&owner)
                .into_iter()
                .flat_map(|definitions| definitions.iter())
                .filter(|definition| definition.file_id != file_id)
                .cloned()
                .collect();
            definitions.extend(
                replacements
                    .iter()
                    .filter(|(declaration, _)| declaration.owner == owner)
                    .map(|(declaration, signature)| FieldDefinition {
                        name: Arc::clone(&declaration.name),
                        file_id,
                        signature: *signature,
                    }),
            );
            definitions.sort_unstable_by(|left, right| {
                left.name
                    .cmp(&right.name)
                    .then_with(|| left.file_id.as_u32().cmp(&right.file_id.as_u32()))
            });
            if definitions.is_empty() {
                self.fields.remove(&owner);
            } else {
                self.fields.insert(owner, definitions.into());
            }
        }
        if replacement_keys.is_empty() {
            self.file_fields.remove(&file_id);
        } else {
            self.file_fields.insert(file_id, replacement_keys);
        }
    }

    fn hierarchies_match(&self, file_id: FileId, replacements: &HierarchyReplacements) -> bool {
        let old_owners = self
            .file_hierarchy_owners
            .get(&file_id)
            .cloned()
            .unwrap_or_default();
        old_owners.len() == replacements.len()
            && old_owners.iter().all(|owner| {
                replacements.get(owner).is_some_and(|replacement| {
                    self.type_hierarchies.get(owner).and_then(|definitions| {
                        definitions
                            .iter()
                            .find(|definition| definition.file_id == file_id)
                            .map(|definition| &definition.hierarchy)
                    }) == Some(replacement)
                })
            })
    }

    fn replace_hierarchies(&mut self, file_id: FileId, replacements: HierarchyReplacements) {
        let old_owners = self
            .file_hierarchy_owners
            .get(&file_id)
            .cloned()
            .unwrap_or_default();
        for owner in &old_owners {
            let definitions: Vec<_> = self
                .type_hierarchies
                .get(owner)
                .into_iter()
                .flat_map(|definitions| definitions.iter())
                .filter(|definition| definition.file_id != file_id)
                .cloned()
                .collect();
            if definitions.is_empty() {
                self.type_hierarchies.remove(owner);
            } else {
                self.type_hierarchies
                    .insert(Arc::clone(owner), definitions.into());
            }
        }
        let replacement_owners: PersistentHashSet<_> = replacements.keys().cloned().collect();
        for (owner, hierarchy) in replacements {
            let mut definitions: Vec<_> = self
                .type_hierarchies
                .get(&owner)
                .into_iter()
                .flat_map(|definitions| definitions.iter())
                .cloned()
                .collect();
            definitions.push(HierarchyDefinition { file_id, hierarchy });
            definitions.sort_unstable_by_key(|definition| definition.file_id.as_u32());
            self.type_hierarchies.insert(owner, definitions.into());
        }
        if replacement_owners.is_empty() {
            self.file_hierarchy_owners.remove(&file_id);
        } else {
            self.file_hierarchy_owners
                .insert(file_id, replacement_owners);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldDefinition {
    name: Arc<str>,
    file_id: FileId,
    signature: FieldSignature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HierarchyDefinition {
    file_id: FileId,
    hierarchy: DirectHierarchy,
}

/// Compact role-preserving hierarchy. When present, the superclass occupies
/// slot zero; all remaining entries are interfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectHierarchy {
    supertypes: Arc<[SupertypeCandidates]>,
    has_superclass: bool,
    /// Number of package segments in `TypeHierarchyFact::owner`.
    ///
    /// This occupies padding already required by the `Arc` and lets field
    /// lookup prove that package-private inheritance stayed in one package at
    /// every superclass edge. Inferring the boundary from capitalization or
    /// dots would confuse member types with packages.
    package_segments: u8,
}

impl DirectHierarchy {
    fn superclass(&self) -> Option<&SupertypeCandidates> {
        self.has_superclass.then(|| &self.supertypes[0])
    }

    fn interfaces(&self) -> &[SupertypeCandidates] {
        &self.supertypes[usize::from(self.has_superclass)..]
    }

    fn all(&self) -> &[SupertypeCandidates] {
        &self.supertypes
    }
}

enum FieldResolution {
    Missing,
    Found(Arc<str>, FieldSignature),
    Ambiguous,
}

#[derive(Clone, Copy)]
struct FieldAccessContext<'a> {
    access_package: Option<&'a str>,
    access_owner: &'a str,
    qualifier_owner: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FieldDeclaration {
    owner: Arc<str>,
    name: Arc<str>,
}

/// Direct Java supertypes for one declared owner. Candidate resolution is
/// deferred until the immutable declaration revision contains all workspace
/// types, so wildcard imports never force a guessed owner during extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[doc(hidden)]
pub struct TypeHierarchyFact {
    owner: String,
    hierarchy: DirectHierarchy,
}

/// Provenance of hierarchy and declared-member facts derived from the
/// workspace index.
///
/// Source bytes alone are not a sufficient revision identity: an unchanged
/// `extends Base` can resolve differently after another declaration is
/// published. Both tokens are therefore part of the cache key. The
/// fingerprint makes the derived hierarchy value inspectable in tests and
/// guards against accidentally constructing the provenance from a different
/// resolved batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[doc(hidden)]
pub struct WorkspaceFactRevision {
    file_revision: FileRevisionToken,
    workspace_revision: WorkspaceRevision,
}

impl WorkspaceFactRevision {
    #[doc(hidden)]
    pub const fn new(
        file_revision: FileRevisionToken,
        workspace_revision: WorkspaceRevision,
    ) -> Self {
        Self {
            file_revision,
            workspace_revision,
        }
    }

    #[doc(hidden)]
    pub const fn file_revision(self) -> FileRevisionToken {
        self.file_revision
    }

    #[doc(hidden)]
    pub const fn workspace_revision(self) -> WorkspaceRevision {
        self.workspace_revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[doc(hidden)]
pub struct WorkspaceMemberFactRevision {
    publication: WorkspaceFactRevision,
    hierarchy_fingerprint: u64,
}

impl WorkspaceMemberFactRevision {
    #[doc(hidden)]
    pub const fn new(
        file_revision: FileRevisionToken,
        workspace_revision: WorkspaceRevision,
        hierarchy_fingerprint: u64,
    ) -> Self {
        Self {
            publication: WorkspaceFactRevision::new(file_revision, workspace_revision),
            hierarchy_fingerprint,
        }
    }

    #[doc(hidden)]
    pub const fn file_revision(self) -> FileRevisionToken {
        self.publication.file_revision
    }

    #[doc(hidden)]
    pub const fn workspace_revision(self) -> WorkspaceRevision {
        self.publication.workspace_revision
    }

    #[doc(hidden)]
    pub const fn publication(self) -> WorkspaceFactRevision {
        self.publication
    }

    #[doc(hidden)]
    pub const fn hierarchy_fingerprint(self) -> u64 {
        self.hierarchy_fingerprint
    }
}

impl TypeHierarchyFact {
    /// Build a hierarchy fact from declaration FQNs already resolved by the
    /// workspace's canonical Java name resolver. Each edge is definitive; no
    /// import or lexical fallback remains for the semantics snapshot to guess.
    #[doc(hidden)]
    pub fn from_resolved_supertype_roles(
        owner: String,
        superclass: Option<String>,
        interfaces: Vec<String>,
    ) -> Self {
        let has_superclass = superclass.is_some();
        Self {
            owner,
            hierarchy: DirectHierarchy {
                supertypes: superclass
                    .into_iter()
                    .chain(interfaces)
                    .map(|supertype| SupertypeCandidates {
                        preferred: Arc::from([Arc::from(supertype)]),
                        on_demand: Arc::from([]),
                    })
                    .collect(),
                has_superclass,
                // Exact package depth is filled from the complete per-file
                // declared-type set when this fact is published.
                package_segments: u8::MAX,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SupertypeCandidates {
    /// Lexically ordered candidates (enclosing member types, explicit import,
    /// current package, or default package). The first published type wins.
    preferred: Arc<[Arc<str>]>,
    /// On-demand imports, including java.lang. More than one published match
    /// is ambiguous and therefore unusable for exact inherited facts.
    on_demand: Arc<[Arc<str>]>,
}

impl FieldDeclaration {
    fn from_fqn(fqn: String) -> Option<Self> {
        let (owner, name) = fqn.rsplit_once('.')?;
        Some(Self {
            owner: Arc::from(owner),
            name: Arc::from(name),
        })
    }
}

fn field_definitions_named<'a>(
    definitions: &'a [FieldDefinition],
    name: &str,
) -> std::slice::Iter<'a, FieldDefinition> {
    let start = definitions.partition_point(|definition| definition.name.as_ref() < name);
    let end =
        start + definitions[start..].partition_point(|definition| definition.name.as_ref() == name);
    definitions[start..end].iter()
}

type VarargVariant = (Arc<str>, FileId, usize);
type VarargsByMethod = PersistentHashMap<Arc<str>, PersistentHashSet<VarargVariant>>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedCallBindings {
    revision: IndexedCallBindingRevision,
    declaration_fqns_by_name_offset: PersistentHashMap<u32, Arc<str>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexedCallBindingRevision {
    LegacyContentHash(u64),
    Workspace(WorkspaceFactRevision),
}

/// Provenance and completeness are one value so a file can never be marked
/// hydrated independently of the revision whose facts were installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishedFileRevision {
    SignaturesOnlyUnversioned,
    SignaturesOnlyContentHash(u64),
    /// Complete syntax facts without workspace-derived provenance. This is
    /// useful to isolated semantic clients, but cannot suppress exact LSP
    /// hydration.
    JavaDeclaredMembersUnversioned,
    /// Compatibility provenance for non-contextual callers. Hash identity is
    /// not exact enough for workspace hierarchy reuse.
    JavaDeclaredMembersContentHash(u64),
    /// Named types, methods, fields, and hierarchy edges from one coherent
    /// workspace resolution batch.
    JavaDeclaredMembersWorkspace(WorkspaceMemberFactRevision),
}

impl PublishedFileRevision {
    const fn signatures_only(content_hash: Option<u64>) -> Self {
        match content_hash {
            Some(content_hash) => Self::SignaturesOnlyContentHash(content_hash),
            None => Self::SignaturesOnlyUnversioned,
        }
    }

    const fn java_declared_members(content_hash: Option<u64>) -> Self {
        match content_hash {
            Some(content_hash) => Self::JavaDeclaredMembersContentHash(content_hash),
            None => Self::JavaDeclaredMembersUnversioned,
        }
    }

    const fn exact_java_declared_members(revision: WorkspaceMemberFactRevision) -> Self {
        Self::JavaDeclaredMembersWorkspace(revision)
    }

    const fn matching_call_binding_revision(self) -> Option<IndexedCallBindingRevision> {
        match self {
            Self::JavaDeclaredMembersWorkspace(revision) => {
                Some(IndexedCallBindingRevision::Workspace(revision.publication))
            }
            Self::SignaturesOnlyContentHash(content_hash)
            | Self::JavaDeclaredMembersContentHash(content_hash) => {
                Some(IndexedCallBindingRevision::LegacyContentHash(content_hash))
            }
            Self::SignaturesOnlyUnversioned | Self::JavaDeclaredMembersUnversioned => None,
        }
    }
}

impl MethodSignatureSnapshot {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            version: 0,
            signatures: PersistentHashMap::new(),
            file_signatures: PersistentHashMap::new(),
            declaration_facts: Arc::new(DeclarationFacts::default()),
            known_files: PersistentHashMap::new(),
            indexed_call_bindings: PersistentHashMap::new(),
            call_variants: PersistentHashMap::new(),
            call_summaries: PersistentHashMap::new(),
            vararg_variants: PersistentHashMap::new(),
            declaration_aliases: PersistentHashMap::new(),
        }
    }

    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    #[must_use]
    pub fn get_signature(&self, fqn: &str) -> Option<MethodSignature> {
        let definitions = self.signatures.get(fqn)?;
        let mut definitions = definitions.values();
        let first = definitions.next()?.clone();
        definitions
            .all(|definition| {
                definition.return_nullability == first.return_nullability
                    && definition.param_nullabilities == first.param_nullabilities
                    && definition.is_static == first.is_static
                    && definition.return_value_kind == first.return_value_kind
                    && definition.invocation_effect == first.invocation_effect
            })
            .then_some(first)
    }

    /// Return a conservative summary for every overload sharing an owner,
    /// method name, and arity. If overload selection cannot distinguish two
    /// annotations, their lattice join is safe for both dereference and
    /// non-null-argument diagnostics.
    #[must_use]
    pub(crate) fn get_call_summary(&self, key: &MethodCallKey) -> Option<MethodContract<'_>> {
        match (
            self.call_summaries.get(key),
            self.expanded_vararg_summary(key),
        ) {
            (Some(exact), Some(vararg)) => Some(MethodContract::owned(join_contract_summaries(
                exact.clone(),
                vararg,
            ))),
            (Some(summary), None) => Some(MethodContract::borrowed(summary)),
            (None, Some(summary)) => Some(MethodContract::owned(summary)),
            (None, None) => None,
        }
    }

    #[must_use]
    pub(crate) fn contains_call(&self, key: &MethodCallKey) -> bool {
        self.call_variants.contains_key(key) || self.has_compatible_vararg(key)
    }

    /// Return the member facts common to every exact-arity declaration that
    /// can satisfy an unindexed source call.
    ///
    /// This is deliberately weaker than workspace overload resolution. Mixed
    /// static/instance overloads remain unresolved, value/conversion kinds are
    /// degraded to `Unknown` when declarations disagree, and effects are
    /// joined toward the broadest exceptional lane. That lets syntax-only
    /// callers retain a proven class-initialization-only effect without
    /// inventing a selected overload.
    pub(crate) fn conservative_call_member_facts(
        &self,
        key: &MethodCallKey,
    ) -> Option<(bool, ValueKind, InvocationEffect, Vec<ValueKind>)> {
        let variants = self.call_variants.get(key)?;
        let facts_for = |(fqn, file_id): &(Arc<str>, FileId)| {
            let signature = self.signatures.get(fqn)?.get(file_id)?;
            Some((
                signature.is_static(),
                signature.return_value_kind(),
                signature.invocation_effect(),
                declaration_parameter_value_kinds(fqn, key.arity)?,
            ))
        };
        let mut variants = variants.iter();
        let (is_static, mut value_kind, mut effect, mut parameter_kinds) =
            facts_for(variants.next()?)?;
        for variant in variants {
            let (candidate_static, candidate_kind, candidate_effect, candidate_parameters) =
                facts_for(variant)?;
            if candidate_static != is_static || candidate_parameters.len() != parameter_kinds.len()
            {
                return None;
            }
            if candidate_kind != value_kind {
                value_kind = ValueKind::Unknown;
            }
            effect = join_invocation_effect(effect, candidate_effect);
            for (kind, candidate) in parameter_kinds.iter_mut().zip(candidate_parameters) {
                if *kind != candidate {
                    *kind = ValueKind::Unknown;
                }
            }
        }
        Some((is_static, value_kind, effect, parameter_kinds))
    }

    pub(crate) fn contains_declaring_type(&self, fqn: &str) -> bool {
        self.declaration_facts
            .declared_type_counts
            .contains_key(fqn)
    }

    pub(crate) fn contains_field(&self, owner: &str, name: &str) -> bool {
        self.declaration_facts
            .fields
            .get(owner)
            .is_some_and(|fields| field_definitions_named(fields, name).next().is_some())
    }

    #[cfg(test)]
    pub(crate) fn field_signature(&self, owner: &str, name: &str) -> Option<FieldSignature> {
        self.raw_field_signature(owner, name)
            .filter(|signature| signature.is_public())
    }

    fn raw_field_signature(&self, owner: &str, name: &str) -> Option<FieldSignature> {
        let mut definitions =
            field_definitions_named(self.declaration_facts.fields.get(owner)?, name);
        let first = definitions.next()?.signature;
        definitions
            .all(|definition| definition.signature == first)
            .then_some(first)
    }

    /// Resolve one exact field declaration through the published Java source
    /// hierarchy. Ambiguous duplicate declarations, wildcard type names, and
    /// interface diamonds remain unresolved instead of inventing an owner.
    #[cfg(test)]
    pub(crate) fn resolve_field_signature(
        &self,
        owner: &str,
        name: &str,
    ) -> Option<(Arc<str>, FieldSignature)> {
        let mut visited = std::collections::HashSet::new();
        match self.resolve_inherited_field(owner, name, true, None, &mut visited) {
            FieldResolution::Found(owner, signature) => Some((owner, signature)),
            FieldResolution::Missing | FieldResolution::Ambiguous => None,
        }
    }

    /// Resolve a field using the exact source access context.
    ///
    /// `access_package` is the parsed package declaration for the current
    /// source file. `access_owner` and `qualifier_owner` are canonical owner
    /// FQNs. Cross-package protected instance access is accepted only when the
    /// accessing class is a subtype of the declaring class and the qualifier's
    /// static type is a subtype of the accessing class. Protected static access
    /// intentionally has no qualifier-type restriction.
    pub(crate) fn resolve_field_signature_from(
        &self,
        owner: &str,
        name: &str,
        access_package: Option<&str>,
        access_owner: &str,
    ) -> Option<(Arc<str>, FieldSignature)> {
        let context = FieldAccessContext {
            access_package,
            access_owner,
            qualifier_owner: owner,
        };
        let mut visited = std::collections::HashSet::new();
        match self.resolve_inherited_field(owner, name, true, Some(context), &mut visited) {
            FieldResolution::Found(owner, signature) => Some((owner, signature)),
            FieldResolution::Missing | FieldResolution::Ambiguous => None,
        }
    }

    /// Resolve an explicitly `super`-qualified field from the direct
    /// superclass of `access_owner`.
    ///
    /// Starting at `access_owner` would incorrectly select a shadowing field
    /// declared by the current class. The qualifier still has the accessing
    /// class's receiver type for Java's cross-package protected rule.
    pub(crate) fn resolve_super_field_signature_from(
        &self,
        access_owner: &str,
        name: &str,
        access_package: Option<&str>,
    ) -> Option<(Arc<str>, FieldSignature)> {
        let hierarchy = self.unique_hierarchy(access_owner).ok()??;
        let superclass = self.resolve_supertype(hierarchy.superclass()?)?;
        let context = FieldAccessContext {
            access_package,
            access_owner,
            qualifier_owner: access_owner,
        };
        let mut visited = std::collections::HashSet::new();
        let mut class_chain = vec![(Arc::from(access_owner), hierarchy.package_segments)];
        let mut interface_roots = Vec::new();
        let class_result = self.resolve_class_chain_field(
            &superclass,
            name,
            false,
            Some(context),
            &mut visited,
            &mut class_chain,
            &mut interface_roots,
        );
        let result = if matches!(class_result, FieldResolution::Missing) {
            let mut interface_visited = std::collections::HashSet::new();
            self.resolve_interface_fields(
                interface_roots,
                name,
                Some(context),
                &mut interface_visited,
            )
        } else {
            class_result
        };
        match result {
            FieldResolution::Found(owner, signature) => Some((owner, signature)),
            FieldResolution::Missing | FieldResolution::Ambiguous => None,
        }
    }

    fn resolve_inherited_field(
        &self,
        owner: &str,
        name: &str,
        is_root: bool,
        access: Option<FieldAccessContext<'_>>,
        visited: &mut std::collections::HashSet<Arc<str>>,
    ) -> FieldResolution {
        let mut interface_roots = Vec::new();
        let mut class_chain = Vec::new();
        let class_result = self.resolve_class_chain_field(
            owner,
            name,
            is_root,
            access,
            visited,
            &mut class_chain,
            &mut interface_roots,
        );
        if !matches!(class_result, FieldResolution::Missing) {
            return class_result;
        }
        let mut interface_visited = std::collections::HashSet::new();
        self.resolve_interface_fields(interface_roots, name, access, &mut interface_visited)
    }

    fn resolve_class_chain_field(
        &self,
        owner: &str,
        name: &str,
        is_root: bool,
        access: Option<FieldAccessContext<'_>>,
        visited: &mut std::collections::HashSet<Arc<str>>,
        class_chain: &mut Vec<(Arc<str>, u8)>,
        interface_roots: &mut Vec<Arc<str>>,
    ) -> FieldResolution {
        let owner: Arc<str> = Arc::from(owner);
        if !visited.insert(Arc::clone(&owner)) {
            return FieldResolution::Missing;
        }
        if let Some(result) =
            self.declared_field_resolution(&owner, name, is_root, access, class_chain)
        {
            return result;
        }
        let hierarchy = match self.unique_hierarchy(&owner) {
            Ok(Some(hierarchy)) => hierarchy,
            Ok(None) => return FieldResolution::Missing,
            Err(()) => return FieldResolution::Ambiguous,
        };
        for interface in hierarchy.interfaces() {
            let Some(interface) = self.resolve_supertype(interface) else {
                return FieldResolution::Ambiguous;
            };
            interface_roots.push(interface);
        }
        let Some(superclass) = hierarchy.superclass() else {
            return FieldResolution::Missing;
        };
        let Some(superclass) = self.resolve_supertype(superclass) else {
            return FieldResolution::Ambiguous;
        };
        class_chain.push((Arc::clone(&owner), hierarchy.package_segments));
        self.resolve_class_chain_field(
            &superclass,
            name,
            false,
            access,
            visited,
            class_chain,
            interface_roots,
        )
    }

    fn resolve_interface_fields(
        &self,
        pending: Vec<Arc<str>>,
        name: &str,
        access: Option<FieldAccessContext<'_>>,
        visited: &mut std::collections::HashSet<Arc<str>>,
    ) -> FieldResolution {
        let mut pending = pending;
        let mut declarations = Vec::new();
        while let Some(owner) = pending.pop() {
            if !visited.insert(Arc::clone(&owner)) {
                continue;
            }
            if let Some(result) = self.declared_field_resolution(&owner, name, false, access, &[]) {
                match result {
                    FieldResolution::Missing => {}
                    FieldResolution::Found(owner, signature) => {
                        declarations.push((owner, signature));
                    }
                    FieldResolution::Ambiguous => return FieldResolution::Ambiguous,
                }
                continue;
            }
            let hierarchy = match self.unique_hierarchy(&owner) {
                Ok(Some(hierarchy)) => hierarchy,
                Ok(None) => continue,
                Err(()) => return FieldResolution::Ambiguous,
            };
            for supertype in hierarchy.all() {
                let Some(supertype) = self.resolve_supertype(supertype) else {
                    return FieldResolution::Ambiguous;
                };
                pending.push(supertype);
            }
        }
        declarations.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        declarations.dedup();
        match declarations.as_slice() {
            [] => FieldResolution::Missing,
            [(owner, signature)] => FieldResolution::Found(Arc::clone(owner), *signature),
            _ => FieldResolution::Ambiguous,
        }
    }

    fn declared_field_resolution(
        &self,
        owner: &Arc<str>,
        name: &str,
        is_root: bool,
        access: Option<FieldAccessContext<'_>>,
        class_chain: &[(Arc<str>, u8)],
    ) -> Option<FieldResolution> {
        if !self.contains_field(owner, name) {
            return None;
        }
        let Some(signature) = self.raw_field_signature(owner, name) else {
            return Some(FieldResolution::Ambiguous);
        };
        Some(match access {
            Some(access) => {
                if self.field_is_accessible(owner, signature, is_root, access, class_chain) {
                    FieldResolution::Found(Arc::clone(owner), signature)
                } else {
                    FieldResolution::Missing
                }
            }
            None => match signature.visibility() {
                FieldVisibility::Public => FieldResolution::Found(Arc::clone(owner), signature),
                FieldVisibility::Private => {
                    if is_root {
                        FieldResolution::Ambiguous
                    } else {
                        FieldResolution::Missing
                    }
                }
                FieldVisibility::Package | FieldVisibility::Protected => FieldResolution::Ambiguous,
            },
        })
    }

    fn unique_hierarchy(&self, owner: &str) -> Result<Option<&DirectHierarchy>, ()> {
        let Some(definitions) = self.declaration_facts.type_hierarchies.get(owner) else {
            return Ok(None);
        };
        let Some(first) = definitions.first() else {
            return Ok(None);
        };
        definitions
            .iter()
            .all(|definition| definition.hierarchy == first.hierarchy)
            .then_some(Some(&first.hierarchy))
            .ok_or(())
    }

    fn field_is_accessible(
        &self,
        declaring_owner: &str,
        signature: FieldSignature,
        is_root: bool,
        access: FieldAccessContext<'_>,
        class_chain: &[(Arc<str>, u8)],
    ) -> bool {
        match signature.visibility() {
            FieldVisibility::Public => true,
            FieldVisibility::Private => is_root && access.access_owner == declaring_owner,
            FieldVisibility::Package => {
                let same_package = signature
                    .declaring_package_matches_context(declaring_owner, access.access_package);
                let inherited_through_declaring_package =
                    class_chain.iter().all(|(owner, package_segments)| {
                        exact_owner_packages_match(
                            owner,
                            *package_segments,
                            declaring_owner,
                            signature.package_segments,
                        )
                    });
                same_package && inherited_through_declaring_package
            }
            FieldVisibility::Protected => {
                if signature
                    .declaring_package_matches_context(declaring_owner, access.access_package)
                {
                    return true;
                }
                self.is_exact_subtype(access.access_owner, declaring_owner)
                    && (signature.is_static()
                        || self.is_exact_subtype(access.qualifier_owner, access.access_owner))
            }
        }
    }

    fn is_exact_subtype(&self, subtype: &str, supertype: &str) -> bool {
        if subtype == supertype {
            return true;
        }
        let mut pending = vec![Arc::<str>::from(subtype)];
        let mut visited = std::collections::HashSet::new();
        while let Some(owner) = pending.pop() {
            if !visited.insert(Arc::clone(&owner)) {
                continue;
            }
            let Some(definitions) = self.declaration_facts.type_hierarchies.get(&owner) else {
                continue;
            };
            let Some(first) = definitions.first() else {
                continue;
            };
            if !definitions
                .iter()
                .all(|definition| definition.hierarchy == first.hierarchy)
            {
                return false;
            }
            for candidates in first.hierarchy.all() {
                let Some(parent) = self.resolve_supertype(candidates) else {
                    return false;
                };
                if parent.as_ref() == supertype {
                    return true;
                }
                pending.push(parent);
            }
        }
        false
    }

    fn resolve_supertype(&self, candidates: &SupertypeCandidates) -> Option<Arc<str>> {
        if let Some(preferred) = candidates
            .preferred
            .iter()
            .find(|candidate| self.contains_declaring_type(candidate))
        {
            return Some(Arc::clone(preferred));
        }
        let mut on_demand = candidates
            .on_demand
            .iter()
            .filter(|candidate| self.contains_declaring_type(candidate));
        let selected = on_demand.next()?;
        on_demand.next().is_none().then(|| Arc::clone(selected))
    }

    #[must_use]
    pub(crate) fn indexed_call_owner(&self, file_id: FileId, name_offset: u32) -> Option<&str> {
        method_declaration_owner(self.indexed_call_declaration(file_id, name_offset)?)
    }

    /// Return the contract for the declaration selected by workspace overload
    /// resolution. This avoids joining unrelated same-arity overloads after a
    /// precise binding is already available.
    #[must_use]
    pub(crate) fn indexed_call_contract(
        &self,
        file_id: FileId,
        name_offset: u32,
        invocation_arity: usize,
    ) -> Option<MethodContract<'_>> {
        let declaration_fqn = self.indexed_call_declaration(file_id, name_offset)?;
        self.declaration_contract(declaration_fqn, invocation_arity)
    }

    pub(crate) fn indexed_call_declaration(
        &self,
        file_id: FileId,
        name_offset: u32,
    ) -> Option<&str> {
        self.indexed_call_bindings
            .get(&file_id)?
            .declaration_fqns_by_name_offset
            .get(&name_offset)
            .map(AsRef::as_ref)
    }

    pub(crate) fn indexed_call_is_static(&self, file_id: FileId, name_offset: u32) -> Option<bool> {
        let declaration = self.indexed_call_declaration(file_id, name_offset)?;
        let definitions = self.signatures.get(declaration)?;
        let mut definitions = definitions.values();
        let first = definitions.next()?.is_static;
        definitions
            .all(|definition| definition.is_static == first)
            .then_some(first)
    }

    pub(crate) fn indexed_call_value_kind(
        &self,
        file_id: FileId,
        name_offset: u32,
    ) -> Option<ValueKind> {
        let declaration = self.indexed_call_declaration(file_id, name_offset)?;
        let definitions = self.signatures.get(declaration)?;
        let mut definitions = definitions.values();
        let first = definitions.next()?.return_value_kind();
        definitions
            .all(|definition| definition.return_value_kind() == first)
            .then_some(first)
    }

    pub(crate) fn indexed_call_effect(
        &self,
        file_id: FileId,
        name_offset: u32,
    ) -> Option<InvocationEffect> {
        let declaration = self.indexed_call_declaration(file_id, name_offset)?;
        let definitions = self.signatures.get(declaration)?;
        let mut definitions = definitions.values();
        let first = definitions.next()?.invocation_effect();
        definitions
            .all(|definition| definition.invocation_effect() == first)
            .then_some(first)
    }

    #[cfg(test)]
    pub(crate) fn indexed_call_may_throw(&self, file_id: FileId, name_offset: u32) -> Option<bool> {
        self.indexed_call_effect(file_id, name_offset)
            .map(|effect| !matches!(effect, InvocationEffect::NonThrowing))
    }

    pub(crate) fn indexed_call_parameter_value_kinds(
        &self,
        file_id: FileId,
        name_offset: u32,
        invocation_arity: usize,
    ) -> Option<Vec<ValueKind>> {
        let declaration = self.indexed_call_declaration(file_id, name_offset)?;
        let open = declaration.rfind('(')?;
        let close = declaration.rfind(')')?;
        let parameters = declaration.get(open + 1..close)?;
        let types = (!parameters.is_empty())
            .then(|| parameters.split(',').map(str::trim))
            .into_iter()
            .flatten();
        Some(super::analysis::invocation_parameter_value_kinds(
            types,
            invocation_arity,
        ))
    }

    fn declaration_contract(
        &self,
        declaration_fqn: &str,
        invocation_arity: usize,
    ) -> Option<MethodContract<'_>> {
        let definitions = if let Some(definitions) = self.signatures.get(declaration_fqn) {
            definitions
        } else {
            let alias = simple_declaration_alias(declaration_fqn);
            let mut candidates = self.declaration_aliases.get(alias.as_str())?.iter();
            let candidate = candidates.next()?;
            if candidates.next().is_some() {
                return None;
            }
            self.signatures.get(candidate)?
        };
        let mut definitions = definitions.values();
        let first = definitions.next()?;
        let declared_arity = first.param_nullabilities.len();
        let vararg_minimum = vararg_minimum_arity(declaration_fqn, declared_arity);

        if vararg_minimum.is_some() && invocation_arity != declared_arity {
            let minimum_arity = vararg_minimum?;
            let mut summary = expanded_signature_summary(first, minimum_arity, invocation_arity)?;
            for definition in definitions {
                let variant =
                    expanded_signature_summary(definition, minimum_arity, invocation_arity)?;
                summary = join_contract_summaries(summary, variant);
            }
            return Some(MethodContract::owned(summary));
        }
        if invocation_arity != declared_arity {
            return None;
        }

        let mut summary = None;
        for definition in definitions {
            if summary.is_none()
                && definition.return_nullability == first.return_nullability
                && definition.param_nullabilities == first.param_nullabilities
            {
                continue;
            }
            let current = summary.get_or_insert_with(|| MethodContractSummary {
                return_nullability: first.return_nullability,
                param_nullabilities: first.param_nullabilities.clone(),
            });
            current.return_nullability = current
                .return_nullability
                .join(definition.return_nullability);
            for (state, variant) in current
                .param_nullabilities
                .iter_mut()
                .zip(&definition.param_nullabilities)
            {
                *state = state.join(*variant);
            }
        }
        summary.map_or_else(
            || {
                Some(MethodContract {
                    return_nullability: first.return_nullability,
                    param_nullabilities: Cow::Borrowed(&first.param_nullabilities),
                })
            },
            |summary| Some(MethodContract::owned(summary)),
        )
    }

    fn replace_file(
        &self,
        file_id: FileId,
        new_signatures: Vec<(String, MethodSignature)>,
        revision: PublishedFileRevision,
    ) -> Option<Self> {
        let replacements: HashMap<Arc<str>, _> = new_signatures
            .into_iter()
            .map(|(fqn, signature)| (Arc::from(fqn), signature))
            .collect();
        let old_keys = self
            .file_signatures
            .get(&file_id)
            .cloned()
            .unwrap_or_default();

        if self.known_files.get(&file_id) == Some(&revision)
            && old_keys.len() == replacements.len()
            && old_keys.iter().all(|fqn| {
                replacements.get(fqn).is_some_and(|replacement| {
                    self.signatures
                        .get(fqn)
                        .and_then(|definitions| definitions.get(&file_id))
                        == Some(replacement)
                })
            })
        {
            return None;
        }

        let mut next = self.clone();
        next.known_files.insert(file_id, revision);
        for fqn in &old_keys {
            let mut definitions = next.signatures.get(fqn).cloned().unwrap_or_default();
            if let Some(signature) = definitions.remove(&file_id) {
                if let Some(key) = method_call_key(fqn.as_ref(), &signature) {
                    let mut variants = next.call_variants.get(&key).cloned().unwrap_or_default();
                    variants.remove(&(fqn.clone(), file_id));
                    if variants.is_empty() {
                        next.call_variants.remove(&key);
                    } else {
                        next.call_variants.insert(key.clone(), variants);
                    }
                    next.refresh_call_summary(&key);
                }
                if let Some((owner, method_name, minimum_arity)) =
                    vararg_call_key(fqn.as_ref(), &signature)
                {
                    next.remove_vararg_variant(
                        owner,
                        method_name,
                        &(fqn.clone(), file_id, minimum_arity),
                    );
                }
            }
            if definitions.is_empty() {
                next.signatures.remove(fqn);
                next.remove_declaration_alias(fqn);
            } else {
                next.signatures.insert(fqn.clone(), definitions);
            }
        }

        let mut replacement_keys = PersistentHashSet::new();
        for (fqn, signature) in replacements {
            replacement_keys.insert(Arc::clone(&fqn));
            let mut definitions = next.signatures.get(&fqn).cloned().unwrap_or_default();
            let is_new_declaration = definitions.is_empty();
            definitions.insert(file_id, signature.clone());
            next.signatures.insert(Arc::clone(&fqn), definitions);
            if is_new_declaration {
                next.insert_declaration_alias(Arc::clone(&fqn));
            }

            if let Some(key) = method_call_key(&fqn, &signature) {
                let mut variants = next.call_variants.get(&key).cloned().unwrap_or_default();
                variants.insert((Arc::clone(&fqn), file_id));
                next.call_variants.insert(key.clone(), variants);
                next.refresh_call_summary(&key);
            }
            if let Some((owner, method_name, minimum_arity)) = vararg_call_key(&fqn, &signature) {
                next.insert_vararg_variant(
                    owner,
                    method_name,
                    (Arc::clone(&fqn), file_id, minimum_arity),
                );
            }
        }
        if replacement_keys.is_empty() {
            next.file_signatures.remove(&file_id);
        } else {
            next.file_signatures.insert(file_id, replacement_keys);
        }

        Some(next)
    }

    /// Assemble one complete per-file publication from a single snapshot
    /// clone. Declaration roots are copied at most once, then fields, declared
    /// types, and hierarchy edges mutate that one private transaction before
    /// the caller installs it under the publication lock.
    fn replace_file_revision(
        &self,
        file_id: FileId,
        new_signatures: Vec<(String, MethodSignature)>,
        new_fields: Vec<(String, FieldSignature)>,
        declared_types: Vec<String>,
        new_hierarchies: Vec<TypeHierarchyFact>,
        revision: PublishedFileRevision,
        retain_matching_call_bindings: bool,
    ) -> Option<Self> {
        let field_replacements: FieldReplacements = new_fields
            .into_iter()
            .filter_map(|(fqn, signature)| {
                FieldDeclaration::from_fqn(fqn).map(|declaration| (declaration, signature))
            })
            .collect();
        let type_replacements: PersistentHashSet<Arc<str>> =
            declared_types.into_iter().map(Arc::from).collect();
        let hierarchy_replacements: HierarchyReplacements = new_hierarchies
            .into_iter()
            .map(|mut fact| {
                // A workspace-resolved hierarchy fact deliberately carries
                // only canonical owner names. Recover the exact package/type
                // boundary from the complete declarations in this same
                // atomic publication; the shortest declared owner prefix is
                // the top-level type, and everything before its final segment
                // is the package. Syntax-extracted facts already contain the
                // same value, so normalizing both paths keeps one invariant.
                fact.hierarchy.package_segments =
                    declared_owner_package_segments(&fact.owner, &type_replacements)
                        .unwrap_or(u8::MAX);
                (Arc::from(fact.owner), fact.hierarchy)
            })
            .collect();
        let fields_changed = !self
            .declaration_facts
            .fields_match(file_id, &field_replacements);
        let types_changed = !self
            .declaration_facts
            .declared_types_match(file_id, &type_replacements);
        let hierarchies_changed = !self
            .declaration_facts
            .hierarchies_match(file_id, &hierarchy_replacements);

        let signature_replacement = self.replace_file(file_id, new_signatures, revision);
        let signatures_changed = signature_replacement.is_some();
        let mut next = signature_replacement.unwrap_or_else(|| self.clone());
        if fields_changed || types_changed || hierarchies_changed {
            let facts = Arc::make_mut(&mut next.declaration_facts);
            if fields_changed {
                facts.replace_fields(file_id, field_replacements);
            }
            if types_changed {
                facts.replace_declared_types(file_id, type_replacements);
            }
            if hierarchies_changed {
                facts.replace_hierarchies(file_id, hierarchy_replacements);
            }
        }

        let call_bindings_changed = if retain_matching_call_bindings {
            let expected_revision = revision
                .matching_call_binding_revision()
                .expect("revision retaining call bindings must carry exact provenance");
            let stale = next
                .indexed_call_bindings
                .get(&file_id)
                .is_some_and(|bindings| bindings.revision != expected_revision);
            if stale {
                next.indexed_call_bindings.remove(&file_id);
            }
            stale
        } else {
            next.indexed_call_bindings.remove(&file_id).is_some()
        };
        (signatures_changed
            || fields_changed
            || types_changed
            || hierarchies_changed
            || call_bindings_changed)
            .then_some(next)
    }

    fn without_file(&self, file_id: FileId) -> Option<Self> {
        if !self.known_files.contains_key(&file_id)
            && !self.indexed_call_bindings.contains_key(&file_id)
            && !self
                .declaration_facts
                .file_declared_types
                .contains_key(&file_id)
            && !self.declaration_facts.file_fields.contains_key(&file_id)
            && !self
                .declaration_facts
                .file_hierarchy_owners
                .contains_key(&file_id)
        {
            return None;
        }
        let mut next = self
            .replace_file_revision(
                file_id,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                PublishedFileRevision::signatures_only(None),
                false,
            )
            .unwrap_or_else(|| self.clone());
        next.known_files.remove(&file_id);
        next.indexed_call_bindings.remove(&file_id);
        Some(next)
    }

    fn with_indexed_call_bindings(
        &self,
        file_id: FileId,
        revision: IndexedCallBindingRevision,
        bindings: Vec<(u32, Arc<str>)>,
    ) -> Option<Self> {
        let mut declaration_fqns_by_name_offset = PersistentHashMap::new();
        let mut ambiguous_offsets = PersistentHashSet::new();
        for (offset, declaration_fqn) in bindings {
            let declaration_fqn = self.resolve_declaration_alias(&declaration_fqn);
            if ambiguous_offsets.contains(&offset) {
                continue;
            }
            match declaration_fqns_by_name_offset.get(&offset) {
                None => {
                    declaration_fqns_by_name_offset.insert(offset, declaration_fqn);
                }
                Some(existing) if existing == &declaration_fqn => {}
                Some(_) => {
                    declaration_fqns_by_name_offset.remove(&offset);
                    ambiguous_offsets.insert(offset);
                }
            }
        }
        let replacement = IndexedCallBindings {
            revision,
            declaration_fqns_by_name_offset,
        };
        if self.indexed_call_bindings.get(&file_id) == Some(&replacement) {
            return None;
        }
        let mut next = self.clone();
        next.indexed_call_bindings.insert(file_id, replacement);
        Some(next)
    }

    fn refresh_call_summary(&mut self, key: &MethodCallKey) {
        let Some(variants) = self.call_variants.get(key) else {
            self.call_summaries.remove(key);
            return;
        };
        let mut signatures = variants.iter().filter_map(|(fqn, file_id)| {
            self.signatures
                .get(fqn)
                .and_then(|definitions| definitions.get(file_id))
        });
        let Some(first) = signatures.next() else {
            self.call_summaries.remove(key);
            return;
        };
        let mut summary = MethodContractSummary {
            return_nullability: first.return_nullability,
            param_nullabilities: first.param_nullabilities.clone(),
        };
        for signature in signatures {
            summary.return_nullability = summary
                .return_nullability
                .join(signature.return_nullability);
            for (state, variant) in summary
                .param_nullabilities
                .iter_mut()
                .zip(&signature.param_nullabilities)
            {
                *state = state.join(*variant);
            }
        }
        self.call_summaries.insert(key.clone(), summary);
    }

    fn has_compatible_vararg(&self, key: &MethodCallKey) -> bool {
        self.vararg_variants
            .get(key.class_name.as_str())
            .and_then(|methods| methods.get(key.method_name.as_str()))
            .is_some_and(|variants| {
                variants
                    .iter()
                    .any(|(_, _, minimum_arity)| *minimum_arity <= key.arity)
            })
    }

    fn expanded_vararg_summary(&self, key: &MethodCallKey) -> Option<MethodContractSummary> {
        let variants = self
            .vararg_variants
            .get(key.class_name.as_str())?
            .get(key.method_name.as_str())?;
        let mut signatures = variants
            .iter()
            .filter(|(_, _, minimum_arity)| *minimum_arity <= key.arity)
            .filter_map(|(fqn, file_id, minimum_arity)| {
                let signature = self.signatures.get(fqn)?.get(file_id)?;
                expanded_signature_summary(signature, *minimum_arity, key.arity)
            });
        let mut summary = signatures.next()?;
        for signature in signatures {
            summary.return_nullability = summary
                .return_nullability
                .join(signature.return_nullability);
            for (state, variant) in summary
                .param_nullabilities
                .iter_mut()
                .zip(signature.param_nullabilities)
            {
                *state = state.join(variant);
            }
        }
        Some(summary)
    }

    fn insert_vararg_variant(
        &mut self,
        owner: Arc<str>,
        method_name: Arc<str>,
        variant: VarargVariant,
    ) {
        let mut methods = self
            .vararg_variants
            .get(&owner)
            .cloned()
            .unwrap_or_default();
        let mut variants = methods.get(&method_name).cloned().unwrap_or_default();
        variants.insert(variant);
        methods.insert(method_name, variants);
        self.vararg_variants.insert(owner, methods);
    }

    fn remove_vararg_variant(
        &mut self,
        owner: Arc<str>,
        method_name: Arc<str>,
        variant: &VarargVariant,
    ) {
        let Some(mut methods) = self.vararg_variants.get(&owner).cloned() else {
            return;
        };
        let Some(mut variants) = methods.get(&method_name).cloned() else {
            return;
        };
        variants.remove(variant);
        if variants.is_empty() {
            methods.remove(&method_name);
        } else {
            methods.insert(method_name, variants);
        }
        if methods.is_empty() {
            self.vararg_variants.remove(&owner);
        } else {
            self.vararg_variants.insert(owner, methods);
        }
    }

    fn resolve_declaration_alias(&self, declaration_fqn: &Arc<str>) -> Arc<str> {
        if self.signatures.contains_key(declaration_fqn) {
            return Arc::clone(declaration_fqn);
        }
        let alias = simple_declaration_alias(declaration_fqn);
        let Some(candidates) = self.declaration_aliases.get(alias.as_str()) else {
            return Arc::clone(declaration_fqn);
        };
        let mut candidates = candidates.iter();
        let Some(candidate) = candidates.next() else {
            return Arc::clone(declaration_fqn);
        };
        let Some(second) = candidates.next() else {
            return Arc::clone(candidate);
        };

        let mut compatible = std::iter::once(candidate)
            .chain(std::iter::once(second))
            .chain(candidates)
            .filter(|candidate| canonical_declaration_matches_source(declaration_fqn, candidate));
        let Some(selected) = compatible.next() else {
            return Arc::clone(declaration_fqn);
        };
        if compatible.next().is_some() {
            Arc::clone(declaration_fqn)
        } else {
            Arc::clone(selected)
        }
    }

    fn insert_declaration_alias(&mut self, declaration_fqn: Arc<str>) {
        let alias = Arc::from(simple_declaration_alias(&declaration_fqn));
        let mut candidates = self
            .declaration_aliases
            .get(&alias)
            .cloned()
            .unwrap_or_default();
        candidates.insert(declaration_fqn);
        self.declaration_aliases.insert(alias, candidates);
    }

    fn remove_declaration_alias(&mut self, declaration_fqn: &Arc<str>) {
        let alias = simple_declaration_alias(declaration_fqn);
        let Some(mut candidates) = self.declaration_aliases.get(alias.as_str()).cloned() else {
            return;
        };
        candidates.remove(declaration_fqn);
        if candidates.is_empty() {
            self.declaration_aliases.remove(alias.as_str());
        } else {
            self.declaration_aliases
                .insert(Arc::from(alias), candidates);
        }
    }
}

impl Default for MethodSignatureSnapshot {
    fn default() -> Self {
        Self::empty()
    }
}

impl<'a> MethodContract<'a> {
    fn borrowed(summary: &'a MethodContractSummary) -> Self {
        Self {
            return_nullability: summary.return_nullability,
            param_nullabilities: Cow::Borrowed(&summary.param_nullabilities),
        }
    }

    fn owned(summary: MethodContractSummary) -> Self {
        Self {
            return_nullability: summary.return_nullability,
            param_nullabilities: Cow::Owned(summary.param_nullabilities),
        }
    }
}

// A version is assigned only when a changed state is published. Comparing it
// is O(1), unlike walking every workspace signature during a Salsa input set.
impl PartialEq for MethodSignatureSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version
    }
}

impl Eq for MethodSignatureSnapshot {}

fn method_call_key(fqn: &str, signature: &MethodSignature) -> Option<MethodCallKey> {
    let open_paren = fqn.rfind('(')?;
    let qualified_name = fqn.get(..open_paren)?;
    let (owner, method_name) = qualified_name.rsplit_once('.')?;
    Some(MethodCallKey {
        class_name: owner.to_string(),
        method_name: method_name.to_string(),
        arity: signature.param_nullabilities.len(),
    })
}

fn declaration_parameter_value_kinds(
    declaration_fqn: &str,
    invocation_arity: usize,
) -> Option<Vec<ValueKind>> {
    let open = declaration_fqn.rfind('(')?;
    let close = declaration_fqn.rfind(')')?;
    let parameters = declaration_fqn.get(open + 1..close)?;
    let types = (!parameters.is_empty())
        .then(|| parameters.split(',').map(str::trim))
        .into_iter()
        .flatten();
    Some(super::analysis::invocation_parameter_value_kinds(
        types,
        invocation_arity,
    ))
}

const fn join_invocation_effect(
    left: InvocationEffect,
    right: InvocationEffect,
) -> InvocationEffect {
    match (left, right) {
        (InvocationEffect::Arbitrary, _) | (_, InvocationEffect::Arbitrary) => {
            InvocationEffect::Arbitrary
        }
        (InvocationEffect::ClassInitialization, _) | (_, InvocationEffect::ClassInitialization) => {
            InvocationEffect::ClassInitialization
        }
        (InvocationEffect::NonThrowing, InvocationEffect::NonThrowing) => {
            InvocationEffect::NonThrowing
        }
    }
}

fn join_contract_summaries(
    mut left: MethodContractSummary,
    right: MethodContractSummary,
) -> MethodContractSummary {
    left.return_nullability = left.return_nullability.join(right.return_nullability);
    for (state, variant) in left
        .param_nullabilities
        .iter_mut()
        .zip(right.param_nullabilities)
    {
        *state = state.join(variant);
    }
    left
}

fn expanded_signature_summary(
    signature: &MethodSignature,
    minimum_arity: usize,
    invocation_arity: usize,
) -> Option<MethodContractSummary> {
    if invocation_arity < minimum_arity {
        return None;
    }
    let vararg_nullability = signature.param_nullabilities.last().copied()?;
    let mut parameters = signature.param_nullabilities.get(..minimum_arity)?.to_vec();
    parameters.resize(invocation_arity, vararg_nullability);
    Some(MethodContractSummary {
        return_nullability: signature.return_nullability,
        param_nullabilities: parameters,
    })
}

fn vararg_minimum_arity(fqn: &str, declared_arity: usize) -> Option<usize> {
    let open_paren = fqn.rfind('(')?;
    let close_paren = fqn.rfind(')')?;
    let parameters = fqn.get(open_paren + 1..close_paren)?;
    parameters
        .rsplit(',')
        .next()
        .is_some_and(|parameter| parameter.trim_end().ends_with("..."))
        .then(|| declared_arity.checked_sub(1))?
}

fn vararg_call_key(fqn: &str, signature: &MethodSignature) -> Option<(Arc<str>, Arc<str>, usize)> {
    let open_paren = fqn.rfind('(')?;
    let minimum_arity = vararg_minimum_arity(fqn, signature.param_nullabilities.len())?;
    let qualified_name = &fqn[..open_paren];
    let (owner, method_name) = qualified_name.rsplit_once('.')?;
    Some((Arc::from(owner), Arc::from(method_name), minimum_arity))
}

fn method_declaration_owner(fqn: &str) -> Option<&str> {
    let qualified_method = fqn.get(..fqn.rfind('(')?)?;
    let (owner, _) = qualified_method.rsplit_once('.')?;
    (!owner.is_empty()).then_some(owner)
}

fn simple_declaration_alias(fqn: &str) -> String {
    let Some(open_paren) = fqn.rfind('(') else {
        return fqn.to_string();
    };
    let Some(close_paren) = fqn.rfind(')') else {
        return fqn.to_string();
    };
    let parameters = &fqn[open_paren + 1..close_paren];
    let mut alias = String::with_capacity(fqn.len());
    alias.push_str(&fqn[..=open_paren]);
    for (index, parameter) in parameters.split(',').enumerate() {
        if index > 0 {
            alias.push(',');
        }
        let parameter = parameter.trim();
        let suffix_start = parameter
            .find('[')
            .or_else(|| parameter.find("..."))
            .unwrap_or(parameter.len());
        let (base, suffix) = parameter.split_at(suffix_start);
        alias.push_str(base.rsplit('.').next().unwrap_or(base));
        alias.push_str(suffix);
    }
    alias.push(')');
    alias
}

fn canonical_declaration_matches_source(canonical: &str, source: &str) -> bool {
    let Some((canonical_name, canonical_parameters)) = declaration_name_and_parameters(canonical)
    else {
        return canonical == source;
    };
    let Some((source_name, source_parameters)) = declaration_name_and_parameters(source) else {
        return false;
    };
    canonical_name == source_name
        && canonical_parameters.len() == source_parameters.len()
        && canonical_parameters
            .into_iter()
            .zip(source_parameters)
            .all(|(canonical, source)| canonical_type_matches_source(canonical, source))
}

fn declaration_name_and_parameters(declaration: &str) -> Option<(&str, Vec<&str>)> {
    let open = declaration.rfind('(')?;
    let close = declaration.rfind(')')?;
    let parameters = declaration.get(open + 1..close)?;
    Some((
        declaration.get(..open)?,
        if parameters.is_empty() {
            Vec::new()
        } else {
            parameters.split(',').map(str::trim).collect()
        },
    ))
}

fn canonical_type_matches_source(canonical: &str, source: &str) -> bool {
    let canonical = canonical.trim();
    let canonical_suffix_start = canonical
        .find('[')
        .or_else(|| canonical.find("..."))
        .unwrap_or(canonical.len());
    let (canonical_qualified_base, canonical_suffix) = canonical.split_at(canonical_suffix_start);
    let canonical_base = canonical_qualified_base
        .rsplit('.')
        .next()
        .unwrap_or(canonical_qualified_base);
    let source = source.trim();
    let source_suffix_start = source
        .find('[')
        .or_else(|| source.find("..."))
        .unwrap_or(source.len());
    let (source_base, source_suffix) = source.split_at(source_suffix_start);
    if canonical_suffix != source_suffix {
        return false;
    }
    if canonical == source {
        return true;
    }
    if source_base.contains('.') {
        // A qualified source spelling must match the same suffix; do not use
        // capitalization to guess whether its first segment is a package or a
        // member type. This supports `Outer.Inner` and legal uppercase package
        // names without conflating `foo.String` with `java.lang.String`.
        return canonical_qualified_base == source_base
            || canonical_qualified_base.ends_with(&format!(".{source_base}"));
    }
    canonical_base == source_base || canonical_qualified_base.ends_with(&format!(".{source_base}"))
}

/// Workspace-wide cache of method signatures
///
/// Tracks method signatures across all files to enable cross-file cache invalidation.
/// When a method's nullability annotations change, we bump the version to signal
/// that dependent analyses need to be recomputed.
pub struct MethodSignatureCache {
    state: Arc<RwLock<MethodSignatureSnapshot>>,
    /// Successful all-file hydration transactions are request-scheduling
    /// metadata, not semantic inputs. Keep them out of immutable Salsa
    /// snapshots so every request does not carry another persistent-map root.
    workspace_hydration_revisions: Arc<RwLock<HashMap<FileId, WorkspaceFactRevision>>>,
}

impl MethodSignatureCache {
    /// Create a new empty signature cache
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(MethodSignatureSnapshot::empty())),
            workspace_hydration_revisions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the signature for a method by its FQN
    ///
    /// Returns `None` if the method is not in the cache.
    pub fn get_signature(&self, fqn: &str) -> Option<MethodSignature> {
        self.state.read().get_signature(fqn)
    }

    #[must_use]
    pub fn contains_file(&self, file_id: FileId) -> bool {
        self.state.read().known_files.contains_key(&file_id)
    }

    #[must_use]
    pub fn contains_java_declaration_revision(&self, file_id: FileId, content_hash: u64) -> bool {
        matches!(
            self.state.read().known_files.get(&file_id),
            Some(PublishedFileRevision::JavaDeclaredMembersContentHash(known_hash))
                if *known_hash == content_hash
        )
    }

    /// Return whether complete Java member facts from this exact file and
    /// workspace publication are installed.
    ///
    /// Comparing the workspace token is essential even when the source bytes
    /// and file token are unchanged: a late same-package or wildcard-imported
    /// declaration can rebind a hierarchy edge contextually.
    #[must_use]
    #[doc(hidden)]
    pub fn contains_workspace_member_fact_revision(
        &self,
        file_id: FileId,
        revision: WorkspaceMemberFactRevision,
    ) -> bool {
        revision.file_revision().file_id() == file_id
            && self.state.read().known_files.get(&file_id)
                == Some(&PublishedFileRevision::JavaDeclaredMembersWorkspace(
                    revision,
                ))
    }

    /// Return whether dependency member facts and exact caller bindings were
    /// fully published for this caller/workspace pair.
    ///
    /// This marker is deliberately stricter than the presence of call
    /// bindings alone: bindings can be published while a dependency contract
    /// is unavailable. Only the all-file hydration transaction may install
    /// it, after its final workspace-revision validation.
    #[must_use]
    #[doc(hidden)]
    pub fn contains_workspace_hydration_revision(
        &self,
        file_id: FileId,
        revision: WorkspaceFactRevision,
    ) -> bool {
        if revision.file_revision().file_id() != file_id {
            return false;
        }
        if self.workspace_hydration_revisions.read().get(&file_id) != Some(&revision) {
            return false;
        }
        self.state
            .read()
            .indexed_call_bindings
            .get(&file_id)
            .is_some_and(|bindings| {
                bindings.revision == IndexedCallBindingRevision::Workspace(revision)
            })
    }

    /// Update signatures for a file
    ///
    /// Returns `true` if any signature changed (requiring cache invalidation),
    /// `false` if all signatures remained the same.
    ///
    /// This method replaces the file's signatures and reports whether the
    /// state changed. The database publisher is responsible for bumping the
    /// version and installing the resulting immutable Salsa input.
    pub fn update_file_signatures(
        &self,
        file_id: FileId,
        new_signatures: Vec<(String, MethodSignature)>,
    ) -> bool {
        let mut state = self.state.write();
        let Some(next) = state.replace_file_revision(
            file_id,
            new_signatures,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            PublishedFileRevision::signatures_only(None),
            false,
        ) else {
            return false;
        };
        *state = next;
        true
    }

    /// Atomically publish all signature and type-identity facts for an
    /// editor/watcher source revision.
    ///
    /// Type identities cannot be derived from method keys because a marker or
    /// exception class may declare no methods. Publishing both collections
    /// under one lock prevents readers from observing a half-installed file
    /// revision.
    pub fn update_file_facts(
        &self,
        file_id: FileId,
        new_signatures: Vec<(String, MethodSignature)>,
        declared_types: Vec<String>,
    ) -> bool {
        let mut state = self.state.write();
        let Some(next) = state.replace_file_revision(
            file_id,
            new_signatures,
            Vec::new(),
            declared_types,
            Vec::new(),
            PublishedFileRevision::signatures_only(None),
            false,
        ) else {
            return false;
        };
        *state = next;
        true
    }

    /// Atomically publish every request-facing member/type fact for one live
    /// editor or watcher revision.
    pub fn update_file_member_facts(
        &self,
        file_id: FileId,
        new_signatures: Vec<(String, MethodSignature)>,
        new_fields: Vec<(String, FieldSignature)>,
        declared_types: Vec<String>,
        type_hierarchies: Vec<TypeHierarchyFact>,
    ) -> bool {
        let mut state = self.state.write();
        let Some(next) = state.replace_file_revision(
            file_id,
            new_signatures,
            new_fields,
            declared_types,
            type_hierarchies,
            PublishedFileRevision::java_declared_members(None),
            false,
        ) else {
            return false;
        };
        *state = next;
        true
    }

    /// Atomically publish all member and hierarchy facts derived from one
    /// exact workspace-index resolution batch.
    ///
    /// The file token proves the source publication, while the workspace token
    /// proves the contextual name-resolution generation. Callers must
    /// revalidate both against the workspace immediately before invoking this
    /// method.
    #[allow(clippy::too_many_arguments)]
    #[doc(hidden)]
    pub fn update_workspace_file_member_facts(
        &self,
        file_id: FileId,
        revision: WorkspaceMemberFactRevision,
        new_signatures: Vec<(String, MethodSignature)>,
        new_fields: Vec<(String, FieldSignature)>,
        declared_types: Vec<String>,
        type_hierarchies: Vec<TypeHierarchyFact>,
    ) -> bool {
        if revision.file_revision().file_id() != file_id {
            debug_assert_eq!(revision.file_revision().file_id(), file_id);
            return false;
        }
        let mut state = self.state.write();
        let Some(next) = state.replace_file_revision(
            file_id,
            new_signatures,
            new_fields,
            declared_types,
            type_hierarchies,
            PublishedFileRevision::exact_java_declared_members(revision),
            true,
        ) else {
            return false;
        };
        *state = next;
        true
    }

    /// Publish signatures extracted lazily from one exact workspace content
    /// hash. A later bulk index revision with a different hash is eligible for
    /// hydration again; editor-published revisions remain authoritative.
    pub fn update_indexed_file_signatures(
        &self,
        file_id: FileId,
        content_hash: u64,
        new_signatures: Vec<(String, MethodSignature)>,
    ) -> bool {
        let mut state = self.state.write();
        let Some(next) = state.replace_file_revision(
            file_id,
            new_signatures,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            PublishedFileRevision::signatures_only(Some(content_hash)),
            true,
        ) else {
            return false;
        };
        *state = next;
        true
    }

    /// Atomically publish all signature and type-identity facts extracted from
    /// one exact cold-workspace content revision.
    pub fn update_indexed_file_facts(
        &self,
        file_id: FileId,
        content_hash: u64,
        new_signatures: Vec<(String, MethodSignature)>,
        declared_types: Vec<String>,
    ) -> bool {
        let mut state = self.state.write();
        let Some(next) = state.replace_file_revision(
            file_id,
            new_signatures,
            Vec::new(),
            declared_types,
            Vec::new(),
            PublishedFileRevision::signatures_only(Some(content_hash)),
            true,
        ) else {
            return false;
        };
        *state = next;
        true
    }

    /// Atomically publish all member/type facts extracted from one exact cold
    /// workspace content revision.
    pub fn update_indexed_file_member_facts(
        &self,
        file_id: FileId,
        content_hash: u64,
        new_signatures: Vec<(String, MethodSignature)>,
        new_fields: Vec<(String, FieldSignature)>,
        declared_types: Vec<String>,
        type_hierarchies: Vec<TypeHierarchyFact>,
    ) -> bool {
        let mut state = self.state.write();
        let Some(next) = state.replace_file_revision(
            file_id,
            new_signatures,
            new_fields,
            declared_types,
            type_hierarchies,
            PublishedFileRevision::java_declared_members(Some(content_hash)),
            true,
        ) else {
            return false;
        };
        *state = next;
        true
    }

    /// Publish exact declarations selected by the workspace index for method
    /// name tokens in one source revision. The declaration FQNs remain shared
    /// with the workspace's forward index.
    pub fn update_indexed_call_bindings(
        &self,
        file_id: FileId,
        content_hash: u64,
        bindings: Vec<(u32, Arc<str>)>,
    ) -> bool {
        let mut state = self.state.write();
        let Some(next) = state.with_indexed_call_bindings(
            file_id,
            IndexedCallBindingRevision::LegacyContentHash(content_hash),
            bindings,
        ) else {
            return false;
        };
        *state = next;
        true
    }

    /// Publish declarations selected by the workspace index for one coherent
    /// file and contextual-resolution revision.
    #[doc(hidden)]
    pub fn update_workspace_call_bindings(
        &self,
        file_id: FileId,
        revision: WorkspaceFactRevision,
        bindings: Vec<(u32, Arc<str>)>,
    ) -> bool {
        if revision.file_revision().file_id() != file_id {
            debug_assert_eq!(revision.file_revision().file_id(), file_id);
            return false;
        }
        let mut state = self.state.write();
        let Some(next) = state.with_indexed_call_bindings(
            file_id,
            IndexedCallBindingRevision::Workspace(revision),
            bindings,
        ) else {
            return false;
        };
        *state = next;
        true
    }

    /// Mark the exact all-file hydration transaction complete. The marker is
    /// operational metadata and therefore does not change the semantic cache
    /// version; every semantic fact it covers was published first.
    #[doc(hidden)]
    pub fn mark_workspace_hydration_complete(
        &self,
        file_id: FileId,
        revision: WorkspaceFactRevision,
    ) -> bool {
        if revision.file_revision().file_id() != file_id {
            debug_assert_eq!(revision.file_revision().file_id(), file_id);
            return false;
        }
        let bindings_are_current = self
            .state
            .read()
            .indexed_call_bindings
            .get(&file_id)
            .is_some_and(|bindings| {
                bindings.revision == IndexedCallBindingRevision::Workspace(revision)
            });
        if !bindings_are_current {
            return false;
        }
        self.workspace_hydration_revisions
            .write()
            .insert(file_id, revision)
            != Some(revision)
    }

    /// Retire source offsets when no exact workspace publication is
    /// available. Keeping an older binding would pair a new syntax tree with a
    /// declaration selected for a different source or resolution generation.
    #[doc(hidden)]
    pub fn remove_workspace_call_bindings(&self, file_id: FileId) -> bool {
        self.workspace_hydration_revisions.write().remove(&file_id);
        let mut state = self.state.write();
        if !state
            .indexed_call_bindings
            .get(&file_id)
            .is_some_and(|bindings| {
                matches!(bindings.revision, IndexedCallBindingRevision::Workspace(_))
            })
        {
            return false;
        }
        let mut next = state.clone();
        next.indexed_call_bindings.remove(&file_id);
        *state = next;
        true
    }

    /// Retire only workspace-derived member facts for `file_id`.
    ///
    /// A newer open-buffer publisher can install syntax-only facts while an
    /// older indexed-source hydration is in flight. Fail-closed hydration must
    /// not erase those independently owned facts when it discovers the source
    /// mismatch.
    #[doc(hidden)]
    pub fn remove_workspace_file_member_facts(&self, file_id: FileId) -> bool {
        let mut state = self.state.write();
        if !state.known_files.get(&file_id).is_some_and(|revision| {
            matches!(
                *revision,
                PublishedFileRevision::JavaDeclaredMembersWorkspace(_)
            )
        }) {
            return false;
        }
        let Some(next) = state.without_file(file_id) else {
            return false;
        };
        *state = next;
        true
    }

    /// Remove all signatures for a file
    ///
    /// Returns `true` if any signatures were removed, `false` otherwise.
    /// Used when a file is deleted or needs full re-indexing.
    pub fn remove_file_signatures(&self, file_id: FileId) -> bool {
        self.workspace_hydration_revisions.write().remove(&file_id);
        let mut state = self.state.write();
        let Some(next) = state.without_file(file_id) else {
            return false;
        };
        *state = next;
        true
    }

    /// Get the current cache version
    ///
    /// Downstream caches can store this version and check if it changed
    /// to determine if they need invalidation.
    pub fn version(&self) -> u64 {
        self.state.read().version()
    }

    /// Increment the version counter
    ///
    /// Returns the new version number.
    pub fn bump_version(&self) -> u64 {
        let mut state = self.state.write();
        state.version += 1;
        state.version
    }

    #[must_use]
    pub fn snapshot(&self) -> MethodSignatureSnapshot {
        self.state.read().clone()
    }
}

impl Default for MethodSignatureCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract method signatures from a file's syntax tree
///
/// Returns a list of (FQN, MethodSignature) pairs for all methods in the file.
///
/// FQN format: `package.ClassName.methodName(Type1,Type2)`
///
/// This function:
/// 1. Extracts package name from PACKAGE_DECL
/// 2. Walks top-level and member type declarations with their qualified owner
/// 3. Extracts only methods directly declared by each named type
/// 4. Builds FQN with parameter types for uniqueness
pub fn extract_declared_type_names(root: &SyntaxNode) -> Vec<String> {
    let package = extract_package_from_node(root);
    let mut declared_types = Vec::new();
    for type_node in root
        .children()
        .filter(|node| node.kind().is_type_declaration())
    {
        collect_declared_type_names(&type_node, package.as_deref(), &mut declared_types);
    }
    declared_types
}

/// Extract direct Java source hierarchy facts using lexical/import precedence.
/// Candidate existence is resolved only after the complete immutable
/// declaration revision is published.
#[doc(hidden)]
pub fn extract_type_hierarchy_facts(root: &SyntaxNode) -> Vec<TypeHierarchyFact> {
    let imports = crate::utils::build_imports_from_syntax(root);
    let package = imports.current_package.as_deref();
    let mut facts = Vec::new();
    for type_node in root
        .children()
        .filter(|node| node.kind().is_type_declaration())
    {
        collect_type_hierarchy_facts(&type_node, package, None, &imports, &mut facts);
    }
    facts
}

fn collect_type_hierarchy_facts(
    type_node: &SyntaxNode,
    package: Option<&str>,
    containing_owner: Option<&str>,
    imports: &hyperion_workspace::ImportMap,
    facts: &mut Vec<TypeHierarchyFact>,
) {
    let Some(type_name) = extract_type_name(type_node) else {
        return;
    };
    let owner = containing_owner.map_or_else(
        || package.map_or_else(|| type_name.clone(), |pkg| format!("{pkg}.{type_name}")),
        |containing| format!("{containing}.{type_name}"),
    );
    let superclass = direct_superclass_type(type_node).and_then(|superclass| {
        extract_source_class_name(&superclass)
            .map(|written| supertype_candidates(&written, package, containing_owner, imports))
    });
    let has_superclass = superclass.is_some();
    let direct_supertypes = superclass
        .into_iter()
        .chain(
            direct_interface_nodes(type_node)
                .into_iter()
                .filter_map(|interface| {
                    extract_source_class_name(&interface).map(|written| {
                        supertype_candidates(&written, package, containing_owner, imports)
                    })
                }),
        )
        .collect();
    facts.push(TypeHierarchyFact {
        owner: owner.clone(),
        hierarchy: DirectHierarchy {
            supertypes: direct_supertypes,
            has_superclass,
            package_segments: package
                .map_or(0, |package| package.split('.').count())
                .try_into()
                .unwrap_or(u8::MAX),
        },
    });
    for nested in type_node
        .children()
        .filter(|node| node.kind().is_type_declaration())
    {
        collect_type_hierarchy_facts(&nested, package, Some(&owner), imports, facts);
    }
}

fn supertype_candidates(
    written: &str,
    package: Option<&str>,
    containing_owner: Option<&str>,
    imports: &hyperion_workspace::ImportMap,
) -> SupertypeCandidates {
    let mut preferred = Vec::new();
    let mut on_demand = Vec::new();
    // A multipart spelling needs bound-head/suffix-failure semantics. The
    // syntax-only fallback cannot prove that namespace boundary, so it refuses
    // the edge. Request-facing publication uses the workspace's canonical
    // resolved direct supertypes and does not take this conservative path.
    if written.contains('.') {
        return SupertypeCandidates {
            preferred: Arc::from([]),
            on_demand: Arc::from([]),
        };
    }
    // A member type in an enclosing lexical scope shadows imports. Walk every
    // enclosing owner, not just the immediate one: Base in
    // Outer.Middle.Child can denote Outer.Middle.Base or Outer.Base.
    let mut lexical_owner = containing_owner;
    while let Some(containing) = lexical_owner {
        preferred.push(Arc::from(format!("{containing}.{written}")));
        lexical_owner = containing.rsplit_once('.').and_then(|(parent, _)| {
            (Some(parent) != package && !parent.is_empty()).then_some(parent)
        });
    }
    if containing_owner.is_some() {
        // An enclosing type may inherit a same-name member type. That lookup
        // requires the workspace hierarchy and must shadow imports; falling
        // through here could select a lower-tier ordinary import incorrectly.
        return SupertypeCandidates {
            preferred: preferred.into(),
            on_demand: Arc::from([]),
        };
    }

    // Single-type and single-static member imports occupy one definitive tier.
    // If malformed source imports two different declarations, publish no edge
    // rather than choosing by map/iteration order or falling to a lower tier.
    let mut single_imports = imports
        .explicit
        .get(written)
        .map(String::as_str)
        .into_iter()
        .chain(imports.static_imports_named(written))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    single_imports.sort_unstable();
    single_imports.dedup();
    if !single_imports.is_empty() {
        if single_imports.len() == 1 {
            preferred.push(Arc::from(single_imports.pop().expect("one checked import")));
        }
        return SupertypeCandidates {
            preferred: preferred.into(),
            on_demand: Arc::from([]),
        };
    }

    if let Some(package) = package {
        preferred.push(Arc::from(format!("{package}.{written}")));
    } else {
        preferred.push(Arc::from(written));
    }
    on_demand.extend(
        imports
            .wildcards
            .iter()
            .map(|package| Arc::from(format!("{package}.{written}"))),
    );
    on_demand.extend(
        imports
            .static_wildcards
            .iter()
            .map(|owner| Arc::from(format!("{owner}.{written}"))),
    );
    on_demand.push(Arc::from(format!("java.lang.{written}")));
    let mut seen_preferred = std::collections::HashSet::new();
    preferred.retain(|candidate| seen_preferred.insert(Arc::clone(candidate)));
    on_demand.sort_unstable();
    on_demand.dedup();
    SupertypeCandidates {
        preferred: preferred.into(),
        on_demand: on_demand.into(),
    }
}

fn direct_superclass_type(declaration: &SyntaxNode) -> Option<SyntaxNode> {
    if declaration.kind() != SyntaxKind::CLASS_DECL {
        return None;
    }
    let mut after_extends = false;
    for element in declaration.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::EXTENDS_KW => {
                after_extends = true;
            }
            NodeOrToken::Node(node) if after_extends && node.kind() == SyntaxKind::CLASS_TYPE => {
                return Some(node);
            }
            _ => {}
        }
    }
    None
}

fn direct_interface_nodes(declaration: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut in_clause = false;
    let extends_interfaces = declaration.kind() == SyntaxKind::INTERFACE_DECL;
    let mut interfaces = Vec::new();
    for element in declaration.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::IMPLEMENTS_KW => {
                in_clause = true;
            }
            NodeOrToken::Token(token)
                if extends_interfaces && token.kind() == SyntaxKind::EXTENDS_KW =>
            {
                in_clause = true;
            }
            NodeOrToken::Token(token)
                if matches!(token.kind(), SyntaxKind::PERMITS_KW | SyntaxKind::L_BRACE) =>
            {
                break;
            }
            NodeOrToken::Node(node) if in_clause && node.kind() == SyntaxKind::CLASS_TYPE => {
                interfaces.push(node);
            }
            _ => {}
        }
    }
    interfaces
}

fn collect_declared_type_names(
    type_node: &SyntaxNode,
    owner_prefix: Option<&str>,
    declared_types: &mut Vec<String>,
) {
    let Some(simple_name) = extract_type_name(type_node) else {
        return;
    };
    let qualified_name = owner_prefix.map_or_else(
        || simple_name.clone(),
        |owner| format!("{owner}.{simple_name}"),
    );
    declared_types.push(qualified_name.clone());
    for nested in type_node
        .children()
        .filter(|node| node.kind().is_type_declaration())
    {
        collect_declared_type_names(&nested, Some(&qualified_name), declared_types);
    }
}

pub fn extract_method_signatures(
    root: &SyntaxNode,
    file_id: FileId,
) -> Vec<(String, MethodSignature)> {
    let mut signatures = Vec::new();
    let package = extract_package_from_node(root);

    for type_node in root
        .children()
        .filter(|node| node.kind().is_type_declaration())
    {
        collect_type_method_signatures(
            &type_node,
            package.as_deref(),
            file_id,
            &HashSet::new(),
            &mut signatures,
        );
    }

    signatures
}

/// Extract compact field dispatch/value facts for the same immutable source
/// revision as [`extract_method_signatures`].
#[doc(hidden)]
pub fn extract_field_signatures(
    root: &SyntaxNode,
    _file_id: FileId,
) -> Vec<(String, FieldSignature)> {
    let package = extract_package_from_node(root);
    let Ok(package_segments) = u8::try_from(
        package
            .as_deref()
            .map_or(0, |package| package.split('.').count()),
    ) else {
        // Java permits long qualified names, but an owner whose package cannot
        // be delimited by the compact fact must remain unpublished rather than
        // be assigned a fabricated package identity.
        return Vec::new();
    };
    let mut signatures = Vec::new();
    for type_node in root
        .children()
        .filter(|node| node.kind().is_type_declaration())
    {
        collect_type_field_signatures(
            &type_node,
            package.as_deref(),
            package_segments,
            &HashSet::new(),
            &mut signatures,
        );
    }
    signatures
}

fn collect_type_field_signatures(
    type_node: &SyntaxNode,
    owner_prefix: Option<&str>,
    package_segments: u8,
    enclosing_type_parameters: &HashSet<String>,
    signatures: &mut Vec<(String, FieldSignature)>,
) {
    let Some(type_name) = extract_type_name(type_node) else {
        return;
    };
    let owner = owner_prefix.map_or_else(
        || type_name.clone(),
        |prefix| format!("{prefix}.{type_name}"),
    );
    let fields_are_implicitly_static = matches!(
        type_node.kind(),
        SyntaxKind::INTERFACE_DECL | SyntaxKind::ANNOTATION_DECL
    );
    let fields_are_implicitly_public = fields_are_implicitly_static;
    let mut visible_type_parameters = enclosing_type_parameters.clone();
    visible_type_parameters.extend(declared_type_parameter_names(type_node));

    for field in type_node
        .children()
        .filter(|node| node.kind() == SyntaxKind::FIELD_DECL)
    {
        let Some(base_type) = extract_declared_type(&field) else {
            continue;
        };
        let is_static = fields_are_implicitly_static
            || super::extraction::has_attached_modifier(&field, SyntaxKind::STATIC_KW);
        let visibility = if fields_are_implicitly_public
            || super::extraction::has_attached_modifier(&field, SyntaxKind::PUBLIC_KW)
        {
            FieldVisibility::Public
        } else if super::extraction::has_attached_modifier(&field, SyntaxKind::PROTECTED_KW) {
            FieldVisibility::Protected
        } else if super::extraction::has_attached_modifier(&field, SyntaxKind::PRIVATE_KW) {
            FieldVisibility::Private
        } else {
            FieldVisibility::Package
        };
        let annotations = node_annotation_names(&field);
        for declarator in field_declarators(&field) {
            let declared_nullability =
                if is_primitive_type(&base_type) && declarator.extra_array_dimensions == 0 {
                    Nullability::NonNull
                } else if has_nullable_annotation(&annotations) {
                    Nullability::Nullable
                } else if has_nonnull_annotation(&annotations) {
                    Nullability::NonNull
                } else {
                    Nullability::Unknown
                };
            let value_kind = if base_type.ends_with("[]") || declarator.extra_array_dimensions > 0 {
                ValueKind::Reference
            } else if visible_type_parameters.contains(base_type.trim()) {
                // The runtime value kind depends on the receiver's generic
                // substitution. Until that substitution is published, an
                // erased `T` is not an exact reference fact.
                ValueKind::Unknown
            } else {
                super::analysis::value_kind_for_type_name(&base_type)
            };
            signatures.push((
                format!("{owner}.{}", declarator.name),
                FieldSignature::from_source(
                    value_kind,
                    is_static,
                    visibility,
                    declared_nullability,
                    package_segments,
                ),
            ));
        }
    }

    for nested_type in type_node
        .children()
        .filter(|node| node.kind().is_type_declaration())
    {
        collect_type_field_signatures(
            &nested_type,
            Some(&owner),
            package_segments,
            &visible_type_parameters,
            signatures,
        );
    }
}

fn node_annotation_names(node: &SyntaxNode) -> Vec<String> {
    fn collect(node: &SyntaxNode, output: &mut Vec<String>) {
        if node.kind() == SyntaxKind::ANNOTATION {
            if let Some(name) = super::annotation_name(node) {
                output.push(name);
            }
        }
        for child in node.children() {
            collect(&child, output);
        }
    }

    let mut names = Vec::new();
    if let Some(modifiers) = node
        .prev_sibling()
        .filter(|sibling| sibling.kind() == SyntaxKind::MODIFIER_LIST)
    {
        collect(&modifiers, &mut names);
    }
    collect(node, &mut names);
    names
}

fn collect_type_method_signatures(
    type_node: &SyntaxNode,
    owner_prefix: Option<&str>,
    file_id: FileId,
    enclosing_type_parameters: &HashSet<String>,
    signatures: &mut Vec<(String, MethodSignature)>,
) {
    let Some(type_name) = extract_type_name(type_node) else {
        return;
    };
    let owner = owner_prefix.map_or_else(
        || type_name.clone(),
        |prefix| format!("{prefix}.{type_name}"),
    );
    let mut owner_type_parameters = enclosing_type_parameters.clone();
    owner_type_parameters.extend(declared_type_parameter_names(type_node));

    for method_node in type_node
        .children()
        .filter(|node| node.kind() == SyntaxKind::METHOD_DECL)
    {
        if let Some(method_info) = extract_method_info(&method_node) {
            let mut visible_type_parameters = owner_type_parameters.clone();
            visible_type_parameters.extend(declared_type_parameter_names(&method_node));
            let param_types = extract_param_types(&method_node);
            let fqn = format!("{owner}.{}({})", method_info.name, param_types.join(","));
            let signature = MethodSignature::new(
                method_info.return_nullability,
                method_info
                    .param_nullability
                    .into_iter()
                    .enumerate()
                    .map(|(index, (name, nullability))| {
                        if nullability == Nullability::NonNull
                            && !method_info.explicit_nonnull_params.contains(&name)
                            && !param_types
                                .get(index)
                                .is_some_and(|parameter| is_primitive_type(parameter))
                        {
                            // The body analysis assumes an unannotated
                            // parameter is non-null to suppress platform
                            // noise. That assumption is not a caller-facing
                            // contract and must not cross a file boundary.
                            Nullability::Unknown
                        } else {
                            nullability
                        }
                    })
                    .collect(),
                file_id,
            )
            .with_static(super::extraction::has_attached_modifier(
                &method_node,
                SyntaxKind::STATIC_KW,
            ))
            .with_return_value_kind(extract_declared_type(&method_node).map_or(
                ValueKind::Unknown,
                |declared_type| {
                    if visible_type_parameters.contains(declared_type.trim()) {
                        ValueKind::Unknown
                    } else {
                        super::analysis::value_kind_for_type_name(&declared_type)
                    }
                },
            ))
            .with_invocation_effect(
                super::source_methods::source_method_invocation_effect(&method_node),
            );
            signatures.push((fqn, signature));
        }
    }

    for nested_type in type_node
        .children()
        .filter(|node| node.kind().is_type_declaration())
    {
        collect_type_method_signatures(
            &nested_type,
            Some(&owner),
            file_id,
            &owner_type_parameters,
            signatures,
        );
    }
}

fn declared_type_parameter_names(declaration: &SyntaxNode) -> impl Iterator<Item = String> + '_ {
    declaration
        .children()
        .filter(|child| child.kind() == SyntaxKind::TYPE_PARAM_LIST)
        .flat_map(|parameters| parameters.children())
        .filter(|parameter| parameter.kind() == SyntaxKind::TYPE_PARAM)
        .filter_map(|parameter| {
            parameter
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .find(|token| token.kind().is_identifier())
                .map(|token| token.text().to_string())
        })
}

/// Classify a declared type only when its value representation is invariant
/// across receiver substitutions. An exact type variable (`T`) is deliberately
/// unknown: `Box<Integer>.value` may unbox while `Box<Object>.value` may not.
pub(crate) fn declaration_value_kind(declaration: &SyntaxNode, declared_type: &str) -> ValueKind {
    let declared_type = declared_type.trim();
    if declared_type.ends_with("[]") || declared_type.ends_with("...") {
        return ValueKind::Reference;
    }
    let is_type_parameter = std::iter::once(declaration.clone())
        .chain(declaration.ancestors().skip(1))
        .filter(|scope| {
            scope.kind().is_type_declaration() || scope.kind() == SyntaxKind::METHOD_DECL
        })
        .any(|scope| {
            declared_type_parameter_names(&scope).any(|parameter| parameter == declared_type)
        });
    if is_type_parameter {
        ValueKind::Unknown
    } else {
        super::analysis::value_kind_for_type_name(declared_type)
    }
}

fn is_primitive_type(ty: &str) -> bool {
    matches!(
        ty,
        "boolean" | "byte" | "short" | "int" | "long" | "char" | "float" | "double"
    )
}

/// Extract package name from a syntax tree
fn extract_package_from_node(root: &SyntaxNode) -> Option<String> {
    for child in root.children() {
        if child.kind() == SyntaxKind::PACKAGE_DECL {
            return extract_package_name_from_decl(&child);
        }
    }
    None
}

/// Extract package name from a PACKAGE_DECL node
fn extract_package_name_from_decl(node: &SyntaxNode) -> Option<String> {
    let mut parts = Vec::new();

    for token in node.descendants_with_tokens() {
        if let NodeOrToken::Token(t) = token {
            if t.kind() == SyntaxKind::IDENT {
                parts.push(t.text().to_string());
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

/// Extract the declared name from any named Java type node.
fn extract_type_name(node: &SyntaxNode) -> Option<String> {
    for elem in node.children_with_tokens() {
        if let NodeOrToken::Token(token) = elem {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

/// Extract parameter type names from a method declaration
///
/// Returns erased, source-spelled type names (for example `String`, `int`, or
/// `java.util.List`). Explicit qualification is preserved so two declarations
/// with the same simple parameter name retain distinct identities.
pub(crate) fn extract_param_types(method_node: &SyntaxNode) -> Vec<String> {
    let mut param_types = Vec::new();

    for param_node in method_node
        .children()
        .find(|child| child.kind() == SyntaxKind::PARAM_LIST)
        .into_iter()
        .flat_map(|parameters| parameters.children())
        .filter(|n| n.kind() == SyntaxKind::PARAM)
    {
        if let Some(type_name) = extract_declared_type(&param_node) {
            param_types.push(type_name);
        }
    }

    param_types
}

/// Extract the type name from a parameter node
pub(crate) fn extract_declared_type(param_node: &SyntaxNode) -> Option<String> {
    // Parser structure for parameters:
    //   PARAM -> [MODIFIER_LIST] -> (PRIMITIVE_TYPE|CLASS_TYPE) -> [ARRAY_TYPE] -> IDENT
    //
    // Key insight: ARRAY_TYPE is just the "[]" brackets as a sibling, NOT a wrapper!
    // So for "String[] items":
    //   PARAM
    //     MODIFIER_LIST
    //     CLASS_TYPE (String)
    //     ARRAY_TYPE ([])
    //     IDENT (items)

    let mut base_type: Option<String> = None;
    let mut array_dimensions = 0;
    let mut is_varargs = false;

    for element in param_node.children_with_tokens() {
        match element {
            NodeOrToken::Node(child) if child.kind() == SyntaxKind::PRIMITIVE_TYPE => {
                // Extract primitive type keyword (int, boolean, etc.)
                for token in child.children_with_tokens() {
                    if let NodeOrToken::Token(t) = token {
                        if matches!(
                            t.kind(),
                            SyntaxKind::INT_KW
                                | SyntaxKind::BOOLEAN_KW
                                | SyntaxKind::BYTE_KW
                                | SyntaxKind::SHORT_KW
                                | SyntaxKind::LONG_KW
                                | SyntaxKind::FLOAT_KW
                                | SyntaxKind::DOUBLE_KW
                                | SyntaxKind::CHAR_KW
                        ) {
                            base_type = Some(t.text().to_string());
                        }
                    }
                }
            }
            NodeOrToken::Node(child) if child.kind() == SyntaxKind::CLASS_TYPE => {
                // Preserve explicit qualification and erase generic arguments.
                base_type = extract_source_class_name(&child);
            }
            NodeOrToken::Node(child) if child.kind() == SyntaxKind::ARRAY_TYPE => {
                array_dimensions += 1;
            }
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::ELLIPSIS => {
                is_varargs = true;
            }
            _ => {}
        }
    }

    base_type.map(|mut base| {
        base.push_str(&"[]".repeat(array_dimensions));
        if is_varargs {
            base.push_str("...");
        }
        base
    })
}

/// Extract the erased, source-spelled name from a `CLASS_TYPE` node.
///
/// For qualified names like `java.util.List`, returns `"java.util.List"`
/// For generics like `List<String>`, returns "List" (ignores type arguments)
fn extract_source_class_name(class_type_node: &SyntaxNode) -> Option<String> {
    // CLASS_TYPE structure can be:
    //   CLASS_TYPE -> QUALIFIED_NAME -> IDENT tokens separated by DOT
    //   CLASS_TYPE -> QUALIFIED_NAME -> IDENT tokens + TYPE_ARGS
    // We need to extract identifiers from QUALIFIED_NAME, stopping at TYPE_ARGS

    let mut idents = Vec::new();

    for elem in class_type_node.children_with_tokens() {
        match elem {
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::QUALIFIED_NAME => {
                // Extract all IDENT tokens from QUALIFIED_NAME
                for token_elem in n.children_with_tokens() {
                    if let NodeOrToken::Token(t) = token_elem {
                        if t.kind() == SyntaxKind::IDENT {
                            idents.push(t.text().to_string());
                        }
                    }
                }
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::TYPE_ARGS => {
                // Stop when we hit type arguments - we don't want identifiers from there
                break;
            }
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::IDENT => {
                idents.push(t.text().to_string());
            }
            _ => {}
        }
    }

    (!idents.is_empty()).then(|| idents.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{syntax::SyntaxNode, Lexer, Parser};

    #[test]
    fn signature_facts_and_snapshot_roots_remain_compact() {
        eprintln!(
            "member fact sizes: MethodSignature={} FieldSignature={} FieldDefinition={} FieldDeclaration={} SupertypeCandidates={} DirectHierarchy={} HierarchyDefinition={} TypeHierarchyFact={} WorkspaceFactRevision={} WorkspaceMemberFactRevision={} PublishedFileRevision={} IndexedCallBindings={} DeclarationFacts={} MethodSignatureSnapshot={}",
            std::mem::size_of::<MethodSignature>(),
            std::mem::size_of::<FieldSignature>(),
            std::mem::size_of::<FieldDefinition>(),
            std::mem::size_of::<FieldDeclaration>(),
            std::mem::size_of::<SupertypeCandidates>(),
            std::mem::size_of::<DirectHierarchy>(),
            std::mem::size_of::<HierarchyDefinition>(),
            std::mem::size_of::<TypeHierarchyFact>(),
            std::mem::size_of::<WorkspaceFactRevision>(),
            std::mem::size_of::<WorkspaceMemberFactRevision>(),
            std::mem::size_of::<PublishedFileRevision>(),
            std::mem::size_of::<IndexedCallBindings>(),
            std::mem::size_of::<DeclarationFacts>(),
            std::mem::size_of::<MethodSignatureSnapshot>(),
        );
        assert!(std::mem::size_of::<MethodSignature>() <= 32);
        assert_eq!(std::mem::size_of::<FieldSignature>(), 3);
        assert!(std::mem::size_of::<FieldDefinition>() <= 24);
        assert!(std::mem::size_of::<FieldDeclaration>() <= 32);
        assert!(std::mem::size_of::<SupertypeCandidates>() <= 32);
        assert!(std::mem::size_of::<DirectHierarchy>() <= 24);
        assert!(std::mem::size_of::<HierarchyDefinition>() <= 32);
        assert!(std::mem::size_of::<TypeHierarchyFact>() <= 48);
        assert!(std::mem::size_of::<WorkspaceFactRevision>() <= 24);
        assert!(std::mem::size_of::<WorkspaceMemberFactRevision>() <= 32);
        assert!(std::mem::size_of::<PublishedFileRevision>() <= 40);
        assert!(std::mem::size_of::<IndexedCallBindings>() <= 64);
        assert!(std::mem::size_of::<DeclarationFacts>() <= 144);
        assert!(
            std::mem::size_of::<MethodSignatureSnapshot>() <= 248,
            "snapshot root regressed to {} bytes",
            std::mem::size_of::<MethodSignatureSnapshot>()
        );
    }

    #[test]
    fn test_signature_cache_basic() {
        let cache = MethodSignatureCache::new();
        assert_eq!(cache.version(), 0);

        let file_id = FileId::new(1);
        let sig = MethodSignature::new(Nullability::NonNull, vec![Nullability::Nullable], file_id)
            .with_static(false);

        let signatures = vec![("com.example.User.getName()".to_string(), sig.clone())];
        let changed = cache.update_file_signatures(file_id, signatures);

        assert!(changed);
        if changed {
            cache.bump_version();
        }
        assert_eq!(cache.version(), 1);

        let retrieved = cache.get_signature("com.example.User.getName()");
        assert_eq!(retrieved, Some(sig));
    }

    #[test]
    fn test_signature_cache_update_detection() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(1);

        // First update
        let sig1 = MethodSignature::new(Nullability::Unknown, vec![], file_id).with_static(false);
        let changed = cache
            .update_file_signatures(file_id, vec![("com.example.Test.foo()".to_string(), sig1)]);
        assert!(changed);
        if changed {
            cache.bump_version();
        }
        assert_eq!(cache.version(), 1);

        // Same signature - no change
        let sig2 = MethodSignature::new(Nullability::Unknown, vec![], file_id).with_static(false);
        let changed = cache
            .update_file_signatures(file_id, vec![("com.example.Test.foo()".to_string(), sig2)]);
        assert!(!changed);
        if changed {
            cache.bump_version();
        }
        assert_eq!(cache.version(), 1); // Version unchanged

        // Changed nullability - should detect change
        let sig3 = MethodSignature::new(Nullability::NonNull, vec![], file_id).with_static(false);
        let changed = cache
            .update_file_signatures(file_id, vec![("com.example.Test.foo()".to_string(), sig3)]);
        assert!(changed);
        if changed {
            cache.bump_version();
        }
        assert_eq!(cache.version(), 2);
    }

    #[test]
    fn test_signature_cache_removal() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(1);

        let sig = MethodSignature::new(Nullability::NonNull, vec![], file_id).with_static(false);

        let changed = cache.update_file_signatures(
            file_id,
            vec![("com.example.Test.method()".to_string(), sig)],
        );
        assert!(changed);
        cache.bump_version();
        assert_eq!(cache.version(), 1);

        let removed = cache.remove_file_signatures(file_id);
        assert!(removed);
        cache.bump_version();
        assert_eq!(cache.version(), 2);
        assert!(cache.get_signature("com.example.Test.method()").is_none());
    }

    #[test]
    fn removing_one_duplicate_definition_preserves_the_other() {
        let cache = MethodSignatureCache::new();
        let first_file = FileId::new(1);
        let second_file = FileId::new(2);
        let fqn = "duplicate.Service.load()".to_string();

        for file_id in [first_file, second_file] {
            assert!(cache.update_file_signatures(
                file_id,
                vec![(
                    fqn.clone(),
                    MethodSignature::new(Nullability::Nullable, Vec::new(), file_id)
                        .with_static(false),
                )],
            ));
        }

        assert!(cache.remove_file_signatures(first_file));
        assert_eq!(
            cache
                .get_signature(&fqn)
                .expect("second definition remains")
                .file_id,
            second_file
        );
    }

    #[test]
    fn expanded_varargs_reuse_the_last_parameter_contract() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(7);
        assert!(cache.update_file_signatures(
            file_id,
            vec![(
                "api.Logger.log(String,String...)".to_string(),
                MethodSignature::new(
                    Nullability::Nullable,
                    vec![Nullability::NonNull, Nullability::Nullable],
                    file_id,
                )
                .with_static(false),
            )],
        ));

        let snapshot = cache.snapshot();
        let summary = |arity| {
            snapshot
                .get_call_summary(&MethodCallKey {
                    class_name: "api.Logger".to_string(),
                    method_name: "log".to_string(),
                    arity,
                })
                .unwrap_or_else(|| panic!("missing varargs summary for arity {arity}"))
        };
        assert_eq!(summary(1).param_nullabilities, vec![Nullability::NonNull]);
        assert_eq!(
            summary(2).param_nullabilities,
            vec![Nullability::NonNull, Nullability::Nullable]
        );
        assert_eq!(
            summary(4).param_nullabilities,
            vec![
                Nullability::NonNull,
                Nullability::Nullable,
                Nullability::Nullable,
                Nullability::Nullable
            ]
        );
        assert_eq!(summary(4).return_nullability, Nullability::Nullable);
        assert!(snapshot
            .get_call_summary(&MethodCallKey {
                class_name: "api.Logger".to_string(),
                method_name: "log".to_string(),
                arity: 0,
            })
            .is_none());

        assert!(cache.remove_file_signatures(file_id));
        assert!(!cache.snapshot().contains_call(&MethodCallKey {
            class_name: "api.Logger".to_string(),
            method_name: "log".to_string(),
            arity: 4,
        }));
    }

    #[test]
    fn fixed_and_varargs_contracts_join_when_arity_alone_cannot_select() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(8);
        assert!(cache.update_file_signatures(
            file_id,
            vec![
                (
                    "api.Logger.log(String,String,String)".to_string(),
                    MethodSignature::new(
                        Nullability::NonNull,
                        vec![Nullability::NonNull; 3],
                        file_id,
                    )
                    .with_static(false),
                ),
                (
                    "api.Logger.log(String...)".to_string(),
                    MethodSignature::new(
                        Nullability::Nullable,
                        vec![Nullability::Nullable],
                        file_id,
                    )
                    .with_static(false),
                ),
            ],
        ));

        let snapshot = cache.snapshot();
        let summary = snapshot
            .get_call_summary(&MethodCallKey {
                class_name: "api.Logger".to_string(),
                method_name: "log".to_string(),
                arity: 3,
            })
            .expect("fixed and expanded varargs candidates");
        assert_eq!(summary.return_nullability, Nullability::Nullable);
        assert_eq!(summary.param_nullabilities, vec![Nullability::Nullable; 3]);
    }

    #[test]
    fn indexed_call_bindings_are_revisioned_and_idempotent() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(1);

        assert!(cache.update_indexed_call_bindings(
            file_id,
            11,
            vec![(42, Arc::from("api.Contract.load()"))],
        ));
        assert!(!cache.update_indexed_call_bindings(
            file_id,
            11,
            vec![(42, Arc::from("api.Contract.load()"))],
        ));
        assert_eq!(
            cache
                .snapshot()
                .indexed_call_bindings
                .get(&file_id)
                .map(|owners| owners.revision),
            Some(IndexedCallBindingRevision::LegacyContentHash(11))
        );
        assert_eq!(
            cache.snapshot().indexed_call_owner(file_id, 42),
            Some("api.Contract")
        );
    }

    #[test]
    fn indexed_call_bindings_expose_primitive_conversion_plan() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(1);
        assert!(cache.update_indexed_call_bindings(
            file_id,
            11,
            vec![(42, Arc::from("api.Contract.load(Integer,int...)"))],
        ));

        assert_eq!(
            cache
                .snapshot()
                .indexed_call_parameter_value_kinds(file_id, 42, 4),
            Some(vec![
                ValueKind::BoxedNumeric,
                ValueKind::PrimitiveNumeric,
                ValueKind::PrimitiveNumeric,
                ValueKind::PrimitiveNumeric,
            ])
        );
    }

    #[test]
    fn indexed_call_bindings_expose_exact_invocation_effect() {
        let cache = MethodSignatureCache::new();
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        assert!(cache.update_file_signatures(
            provider,
            vec![(
                "api.Safe.value()".to_owned(),
                MethodSignature::new(Nullability::NonNull, Vec::new(), provider)
                    .with_static(true)
                    .with_invocation_may_throw(false),
            )],
        ));
        assert!(cache.update_indexed_call_bindings(
            caller,
            11,
            vec![(42, Arc::from("api.Safe.value()"))],
        ));

        assert_eq!(
            cache.snapshot().indexed_call_may_throw(caller, 42),
            Some(false)
        );
    }

    #[test]
    fn indexed_call_contracts_preserve_exact_same_arity_overloads() {
        let cache = MethodSignatureCache::new();
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        assert!(cache.update_file_signatures(
            provider,
            vec![
                (
                    "api.Service.load(String)".to_string(),
                    MethodSignature::new(
                        Nullability::Nullable,
                        vec![Nullability::NonNull],
                        provider,
                    )
                    .with_static(false),
                ),
                (
                    "api.Service.load(int)".to_string(),
                    MethodSignature::new(
                        Nullability::NonNull,
                        vec![Nullability::NonNull],
                        provider,
                    )
                    .with_static(false),
                ),
            ],
        ));
        assert!(cache.update_indexed_call_bindings(
            caller,
            11,
            vec![
                (42, Arc::from("api.Service.load(java.lang.String)")),
                (84, Arc::from("api.Service.load(int)")),
            ],
        ));

        let snapshot = cache.snapshot();
        assert_eq!(
            snapshot
                .indexed_call_contract(caller, 42, 1)
                .expect("String overload")
                .return_nullability,
            Nullability::Nullable
        );
        assert_eq!(
            snapshot
                .indexed_call_contract(caller, 84, 1)
                .expect("int overload")
                .return_nullability,
            Nullability::NonNull
        );
    }

    #[test]
    fn ambiguous_simple_descriptor_aliases_are_not_guessed() {
        let cache = MethodSignatureCache::new();
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        assert!(cache.update_file_signatures(
            provider,
            vec![
                (
                    "api.Service.load(left.Value)".to_string(),
                    MethodSignature::new(
                        Nullability::Nullable,
                        vec![Nullability::Unknown],
                        provider,
                    )
                    .with_static(false),
                ),
                (
                    "api.Service.load(right.Value)".to_string(),
                    MethodSignature::new(
                        Nullability::NonNull,
                        vec![Nullability::Unknown],
                        provider,
                    )
                    .with_static(false),
                ),
            ],
        ));
        assert!(cache.update_indexed_call_bindings(
            caller,
            11,
            vec![(42, Arc::from("api.Service.load(other.Value)"))],
        ));

        assert!(cache
            .snapshot()
            .indexed_call_contract(caller, 42, 1)
            .is_none());
    }

    #[test]
    fn canonical_descriptor_disambiguates_colliding_source_type_names() {
        let cache = MethodSignatureCache::new();
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        assert!(cache.update_file_signatures(
            provider,
            vec![
                (
                    "api.Service.load(String)".to_string(),
                    MethodSignature::new(
                        Nullability::Nullable,
                        vec![Nullability::Unknown],
                        provider,
                    ),
                ),
                (
                    "api.Service.load(foo.String)".to_string(),
                    MethodSignature::new(
                        Nullability::NonNull,
                        vec![Nullability::Unknown],
                        provider,
                    ),
                ),
            ],
        ));
        assert!(cache.update_indexed_call_bindings(
            caller,
            11,
            vec![(42, Arc::from("api.Service.load(java.lang.String)"),)],
        ));

        let snapshot = cache.snapshot();
        assert_eq!(
            snapshot.indexed_call_declaration(caller, 42),
            Some("api.Service.load(String)")
        );
        assert_eq!(
            snapshot
                .indexed_call_contract(caller, 42, 1)
                .expect("canonical java.lang.String binding")
                .return_nullability,
            Nullability::Nullable
        );
    }

    #[test]
    fn conflicting_indexed_call_bindings_are_not_guessed() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(1);

        assert!(cache.update_indexed_call_bindings(
            file_id,
            11,
            vec![
                (42, Arc::from("api.First.load()")),
                (42, Arc::from("api.Second.load()")),
            ],
        ));
        assert!(cache.snapshot().indexed_call_owner(file_id, 42).is_none());
    }

    #[test]
    fn indexed_signature_revisions_preserve_only_matching_call_owners() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(1);
        let signature =
            MethodSignature::new(Nullability::Nullable, Vec::new(), file_id).with_static(false);
        let signatures = || vec![("api.Contract.load()".to_string(), signature.clone())];

        assert!(cache.update_indexed_file_signatures(file_id, 11, signatures()));
        assert!(cache.update_indexed_call_bindings(
            file_id,
            11,
            vec![(42, Arc::from("api.Contract.load()"))],
        ));
        assert!(!cache.update_indexed_file_signatures(file_id, 11, signatures()));
        assert_eq!(
            cache.snapshot().indexed_call_owner(file_id, 42),
            Some("api.Contract")
        );

        assert!(cache.update_indexed_file_signatures(file_id, 12, signatures()));
        assert!(!cache
            .snapshot()
            .indexed_call_bindings
            .contains_key(&file_id));
        assert!(cache.snapshot().indexed_call_owner(file_id, 42).is_none());
    }

    #[test]
    fn authoritative_updates_and_removal_clear_indexed_call_bindings() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(1);
        let signature =
            MethodSignature::new(Nullability::Nullable, Vec::new(), file_id).with_static(false);
        let signatures = || vec![("api.Contract.load()".to_string(), signature.clone())];

        assert!(cache.update_file_signatures(file_id, signatures()));
        assert!(cache.update_indexed_call_bindings(
            file_id,
            11,
            vec![(42, Arc::from("api.Contract.load()"))],
        ));
        assert!(cache.update_file_signatures(file_id, signatures()));
        assert!(cache.snapshot().indexed_call_owner(file_id, 42).is_none());

        assert!(cache.update_indexed_call_bindings(
            file_id,
            11,
            vec![(42, Arc::from("api.Contract.load()"))],
        ));
        assert!(cache.remove_file_signatures(file_id));
        assert!(!cache.contains_file(file_id));
        assert!(!cache
            .snapshot()
            .indexed_call_bindings
            .contains_key(&file_id));
    }

    fn parse_java(source: &str) -> SyntaxNode {
        let tokens: Vec<_> = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        SyntaxNode::new_root(green_node)
    }

    #[test]
    fn declared_type_facts_include_methodless_member_types_but_not_locals() {
        let root = parse_java(
            r#"
            package example;
            class Outer {
                class Inner {}
                void body() { class Local {} }
            }
            interface Marker {}
            "#,
        );
        let mut declared_types = extract_declared_type_names(&root);
        declared_types.sort();
        assert_eq!(
            declared_types,
            ["example.Marker", "example.Outer", "example.Outer.Inner"]
        );

        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(1);
        assert!(cache.update_file_facts(file_id, Vec::new(), declared_types));
        let snapshot = cache.snapshot();
        assert!(snapshot.contains_declaring_type("example.Marker"));
        assert!(snapshot.contains_declaring_type("example.Outer.Inner"));
        assert!(!snapshot.contains_declaring_type("example.Outer.Local"));
    }

    #[test]
    fn replacing_and_removing_file_facts_maintains_shared_type_counts() {
        let cache = MethodSignatureCache::new();
        let first = FileId::new(1);
        let second = FileId::new(2);
        assert!(cache.update_file_facts(first, Vec::new(), vec!["example.Shared".to_owned()]));
        assert!(cache.update_file_facts(second, Vec::new(), vec!["example.Shared".to_owned()]));
        assert!(cache.update_file_facts(first, Vec::new(), vec!["example.Replacement".to_owned()]));
        let snapshot = cache.snapshot();
        assert!(snapshot.contains_declaring_type("example.Shared"));
        assert!(snapshot.contains_declaring_type("example.Replacement"));

        assert!(cache.remove_file_signatures(second));
        let snapshot = cache.snapshot();
        assert!(!snapshot.contains_declaring_type("example.Shared"));
        assert!(snapshot.contains_declaring_type("example.Replacement"));
        assert!(cache.remove_file_signatures(first));
        assert!(!cache
            .snapshot()
            .contains_declaring_type("example.Replacement"));
    }

    #[test]
    fn legacy_publication_cannot_masquerade_as_complete_member_hydration() {
        let cache = MethodSignatureCache::new();
        let file_id = FileId::new(1);
        let content_hash = 0x5eed_u64;
        let signature =
            MethodSignature::new(Nullability::NonNull, Vec::new(), file_id).with_static(true);
        let signatures = || vec![("example.Marker.create()".to_owned(), signature.clone())];
        let declared_types = || vec!["example.Marker".to_owned()];

        assert!(cache.update_indexed_file_member_facts(
            file_id,
            content_hash,
            signatures(),
            Vec::new(),
            declared_types(),
            Vec::new(),
        ));
        assert!(cache.contains_java_declaration_revision(file_id, content_hash));
        assert!(cache.snapshot().contains_declaring_type("example.Marker"));

        assert!(cache.update_file_signatures(file_id, signatures()));
        assert!(cache.contains_file(file_id));
        assert!(!cache.contains_java_declaration_revision(file_id, content_hash));
        assert!(!cache.snapshot().contains_declaring_type("example.Marker"));

        assert!(cache.update_indexed_file_facts(
            file_id,
            content_hash,
            signatures(),
            declared_types(),
        ));
        assert!(!cache.contains_java_declaration_revision(file_id, content_hash));
        assert!(cache.snapshot().contains_declaring_type("example.Marker"));

        assert!(cache.update_indexed_file_member_facts(
            file_id,
            content_hash,
            signatures(),
            Vec::new(),
            declared_types(),
            Vec::new(),
        ));
        assert!(cache.contains_java_declaration_revision(file_id, content_hash));

        assert!(cache.update_file_member_facts(
            file_id,
            signatures(),
            Vec::new(),
            declared_types(),
            Vec::new(),
        ));
        assert!(cache.contains_file(file_id));
        assert!(
            !cache.contains_java_declaration_revision(file_id, content_hash),
            "an unversioned live publication must not claim every matching hash"
        );

        assert!(cache.update_file_facts(file_id, signatures(), declared_types()));
        assert!(!cache.contains_java_declaration_revision(file_id, content_hash));
    }

    #[test]
    fn test_extract_method_signatures_simple() {
        let source = r#"
package com.example;

public class User {
    public String getName() {
        return name;
    }

    public void setName(String name) {
        this.name = name;
    }
}
"#;

        let root = parse_java(source);

        let file_id = FileId::new(1);
        let signatures = extract_method_signatures(&root, file_id);

        assert_eq!(signatures.len(), 2);

        // Check getName signature
        let get_name = signatures
            .iter()
            .find(|(fqn, _)| fqn.contains("getName"))
            .unwrap();
        assert_eq!(get_name.0, "com.example.User.getName()");
        assert_eq!(get_name.1.return_nullability, Nullability::Unknown);
        assert_eq!(get_name.1.param_nullabilities.len(), 0);

        // Check setName signature
        let set_name = signatures
            .iter()
            .find(|(fqn, _)| fqn.contains("setName"))
            .unwrap();
        assert_eq!(set_name.0, "com.example.User.setName(String)");
        assert_eq!(set_name.1.return_nullability, Nullability::NonNull); // void -> NonNull
        assert_eq!(set_name.1.param_nullabilities, vec![Nullability::Unknown]);
    }

    #[test]
    fn extracted_invocation_effects_share_the_source_method_proof() {
        let signatures: HashMap<_, _> = extract_method_signatures(
            &parse_java(
                r#"
                package api;
                class Service {
                    static double staticBodySafe() { return 2.0; }
                    private double safe() { return 2.0; }
                    private void assertion() { assert true; }
                    static double risky() { return dependency(); }
                    static native void nativeCall();
                    double overridable() { return 2.0; }
                }
                final class SealedService { double safe() { return 2.0; } }
                "#,
            ),
            FileId::new(1),
        )
        .into_iter()
        .collect();

        assert!(!signatures["api.Service.safe()"].invocation_may_throw());
        assert_eq!(
            signatures["api.Service.staticBodySafe()"].invocation_effect(),
            InvocationEffect::ClassInitialization
        );
        assert!(signatures["api.Service.assertion()"].invocation_may_throw());
        assert!(signatures["api.Service.risky()"].invocation_may_throw());
        assert!(signatures["api.Service.nativeCall()"].invocation_may_throw());
        assert!(signatures["api.Service.overridable()"].invocation_may_throw());
        assert!(!signatures["api.SealedService.safe()"].invocation_may_throw());
    }

    #[test]
    fn extracts_compact_field_dispatch_and_value_facts() {
        let fields: HashMap<_, _> = extract_field_signatures(
            &parse_java(
                r#"
                package api;
                class Values {
                    static double DIVISOR = 2.0;
                    int count = 1, samples[] = null;
                    class Nested { static boolean READY = true; }
                }
                interface Contract { int LIMIT = 10; }
                "#,
            ),
            FileId::new(1),
        )
        .into_iter()
        .collect();

        assert!(fields["api.Values.DIVISOR"].is_static());
        assert_eq!(
            fields["api.Values.DIVISOR"].value_kind(),
            ValueKind::PrimitiveFloating
        );
        assert!(!fields["api.Values.count"].is_static());
        assert_eq!(
            fields["api.Values.count"].value_kind(),
            ValueKind::PrimitiveNumeric
        );
        assert_eq!(
            fields["api.Values.samples"].value_kind(),
            ValueKind::Reference
        );
        assert!(fields["api.Values.Nested.READY"].is_static());
        assert!(fields["api.Contract.LIMIT"].is_static());
        assert!(
            std::mem::size_of::<FieldSignature>() <= 3,
            "field signature grew to {} bytes",
            std::mem::size_of::<FieldSignature>()
        );
    }

    #[test]
    fn field_facts_publish_nullability_and_exact_declaring_package_delimiter() {
        let fields: HashMap<_, _> = extract_field_signatures(
            &parse_java(
                r#"
                package api.deep;
                class Values {
                    @Nullable String optional;
                    @NonNull String required;
                    int primitive;
                }
                "#,
            ),
            FileId::new(1),
        )
        .into_iter()
        .collect();

        assert_eq!(
            fields["api.deep.Values.optional"].nullability(),
            Nullability::Nullable
        );
        assert_eq!(
            fields["api.deep.Values.required"].nullability(),
            Nullability::NonNull
        );
        assert_eq!(
            fields["api.deep.Values.primitive"].nullability(),
            Nullability::NonNull
        );
        assert!(fields["api.deep.Values.optional"]
            .declaring_package_matches("api.deep.Values", "api.deep"));
        assert!(
            !fields["api.deep.Values.optional"].declaring_package_matches("api.deep.Values", "api")
        );
    }

    #[test]
    fn exact_field_access_context_enforces_package_and_protected_rules() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            concat!(
                "package api; public class Base { static String PACKAGE; ",
                "@Nullable protected static String STATIC_VALUE; ",
                "@Nullable protected String instanceValue; }",
            ),
        );
        let child_file = FileId::new(2);
        let child = parse_java("package app; public class Child extends api.Base {}");
        assert!(cache.update_file_member_facts(
            child_file,
            extract_method_signatures(&child, child_file),
            extract_field_signatures(&child, child_file),
            extract_declared_type_names(&child),
            vec![TypeHierarchyFact::from_resolved_supertype_roles(
                "app.Child".to_owned(),
                Some("api.Base".to_owned()),
                Vec::new(),
            )],
        ));
        let snapshot = cache.snapshot();

        assert!(snapshot
            .resolve_field_signature_from("api.Base", "PACKAGE", Some("api"), "api.Client",)
            .is_some());
        assert!(snapshot
            .resolve_field_signature_from("api.Base", "PACKAGE", Some("app"), "app.Child",)
            .is_none());
        assert!(snapshot
            .resolve_field_signature_from("app.Child", "STATIC_VALUE", Some("app"), "app.Child",)
            .is_some());
        assert!(snapshot
            .resolve_field_signature_from("app.Child", "instanceValue", Some("app"), "app.Child",)
            .is_some());
        assert!(snapshot
            .resolve_field_signature_from(
                "api.Base",
                "instanceValue",
                Some("app"),
                "app.Child",
            )
            .is_none(), "a cross-package protected instance receiver must be Child-or-narrower");
    }

    #[test]
    fn extracts_qualified_generic_annotated_and_mixed_dimension_fields() {
        let fields: HashMap<_, _> = extract_field_signatures(
            &parse_java(
                r#"
                package api;
                class Values {
                    @Nullable java.lang.Integer boxed;
                    java.util.List<String> values;
                    int[] x, y[];
                }
                "#,
            ),
            FileId::new(1),
        )
        .into_iter()
        .collect();

        assert_eq!(
            fields["api.Values.boxed"].value_kind(),
            ValueKind::BoxedNumeric
        );
        for field in ["values", "x", "y"] {
            assert_eq!(
                fields[format!("api.Values.{field}").as_str()].value_kind(),
                ValueKind::Reference,
                "wrong value kind for {field}"
            );
        }
    }

    #[test]
    fn erased_type_variables_are_not_published_as_concrete_reference_facts() {
        let root = parse_java(
            r#"
            package api;
            class Box<T> {
                T value;
                T get() { return value; }
                <R> R convert(T input) { return null; }
            }
            "#,
        );
        let fields: HashMap<_, _> = extract_field_signatures(&root, FileId::new(1))
            .into_iter()
            .collect();
        let methods: HashMap<_, _> = extract_method_signatures(&root, FileId::new(1))
            .into_iter()
            .collect();

        assert_eq!(fields["api.Box.value"].value_kind(), ValueKind::Unknown);
        assert_eq!(
            methods["api.Box.get()"].return_value_kind(),
            ValueKind::Unknown
        );
        assert_eq!(
            methods["api.Box.convert(T)"].return_value_kind(),
            ValueKind::Unknown
        );
    }

    fn publish_java_member_facts(cache: &MethodSignatureCache, file: FileId, source: &str) {
        let root = parse_java(source);
        assert!(cache.update_file_member_facts(
            file,
            extract_method_signatures(&root, file),
            extract_field_signatures(&root, file),
            extract_declared_type_names(&root),
            extract_type_hierarchy_facts(&root),
        ));
    }

    #[test]
    fn inherited_fields_resolve_through_classes_and_interfaces() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            "package api; class Base { public static int FROM_BASE = 1; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            "package api; interface Contract { int FROM_INTERFACE = 2; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(3),
            "package impl; import api.*; class Child extends Base implements Contract {}",
        );
        let snapshot = cache.snapshot();

        assert_eq!(
            snapshot
                .resolve_field_signature("impl.Child", "FROM_BASE")
                .map(|(owner, signature)| (owner.to_string(), signature)),
            Some((
                "api.Base".to_owned(),
                FieldSignature::from_source(
                    ValueKind::PrimitiveNumeric,
                    true,
                    FieldVisibility::Public,
                    Nullability::NonNull,
                    1,
                ),
            ))
        );
        assert_eq!(
            snapshot
                .resolve_field_signature("impl.Child", "FROM_INTERFACE")
                .map(|(owner, signature)| (owner.to_string(), signature)),
            Some((
                "api.Contract".to_owned(),
                FieldSignature::from_source(
                    ValueKind::PrimitiveNumeric,
                    true,
                    FieldVisibility::Public,
                    Nullability::NonNull,
                    1,
                ),
            ))
        );
    }

    #[test]
    fn class_chain_field_dominates_same_named_interface_field() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            "package api; interface I { @Nonnull String value = \"safe\"; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            "package api; class B { @Nullable public String value; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(3),
            "package api; class C extends B implements I {}",
        );

        let (owner, signature) = cache
            .snapshot()
            .resolve_field_signature("api.C", "value")
            .expect("class-chain field must dominate the interface declaration");
        assert_eq!(owner.as_ref(), "api.B");
        assert_eq!(signature.nullability(), Nullability::Nullable);
    }

    #[test]
    fn inherited_grandparent_field_dominates_direct_interface_field() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            "package api; class Ancestor { public static int VALUE = 1; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            "package api; class Parent extends Ancestor {}",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(3),
            "package api; interface Contract { int VALUE = 2; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(4),
            "package api; class Child extends Parent implements Contract {}",
        );

        assert_eq!(
            cache
                .snapshot()
                .resolve_field_signature("api.Child", "VALUE")
                .map(|(owner, _)| owner),
            Some(Arc::from("api.Ancestor"))
        );
    }

    #[test]
    fn permitted_subtypes_are_not_published_as_superinterfaces() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            "package api; interface Drawable { int PARENT = 1; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            concat!(
                "package api; sealed interface Shape extends Drawable permits Circle {} ",
                "final class Circle implements Shape { public static int SUBTYPE = 2; }",
            ),
        );
        let snapshot = cache.snapshot();

        assert!(snapshot
            .resolve_field_signature("api.Shape", "PARENT")
            .is_some());
        assert!(snapshot
            .resolve_field_signature("api.Shape", "SUBTYPE")
            .is_none());
    }

    #[test]
    fn nested_lexical_supertypes_shadow_imports_at_every_enclosing_level() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            "package imported; public class Base { public static int WRONG = 1; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            concat!(
                "package app; import imported.Base; class Outer { ",
                "class Base { public static int RIGHT = 2; } ",
                "class Middle { class Child extends Base {} } }",
            ),
        );
        let snapshot = cache.snapshot();

        assert_eq!(
            snapshot
                .resolve_field_signature("app.Outer.Middle.Child", "RIGHT")
                .map(|(owner, _)| owner.to_string()),
            Some("app.Outer.Base".to_owned())
        );
        assert!(snapshot
            .resolve_field_signature("app.Outer.Middle.Child", "WRONG")
            .is_none());
    }

    #[test]
    fn qualified_supertype_resolution_does_not_assume_lowercase_packages() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            "package API; public class Base { public static int VALUE = 1; }",
        );
        let child_file = FileId::new(2);
        let child = parse_java("package app; public class Child extends API.Base {}");
        assert!(cache.update_file_member_facts(
            child_file,
            extract_method_signatures(&child, child_file),
            extract_field_signatures(&child, child_file),
            extract_declared_type_names(&child),
            vec![TypeHierarchyFact::from_resolved_supertype_roles(
                "app.Child".to_owned(),
                Some("API.Base".to_owned()),
                Vec::new(),
            )],
        ));

        assert_eq!(
            cache
                .snapshot()
                .resolve_field_signature("app.Child", "VALUE")
                .map(|(owner, _)| owner.to_string()),
            Some("API.Base".to_owned())
        );
    }

    #[test]
    fn syntax_fallback_respects_single_and_wildcard_static_member_type_imports() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            concat!(
                "package api; public class Types { public static class Base { ",
                "public static int VALUE = 1; } }",
            ),
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            concat!(
                "package one; import static api.Types.Base; ",
                "public class Child extends Base {}",
            ),
        );
        publish_java_member_facts(
            &cache,
            FileId::new(3),
            concat!(
                "package two; import static api.Types.*; ",
                "public class Child extends Base {}",
            ),
        );
        let snapshot = cache.snapshot();

        for owner in ["one.Child", "two.Child"] {
            assert_eq!(
                snapshot
                    .resolve_field_signature(owner, "VALUE")
                    .map(|(owner, _)| owner.to_string()),
                Some("api.Types.Base".to_owned()),
                "static-imported member type must resolve for {owner}"
            );
        }
    }

    #[test]
    fn syntax_fallback_never_falls_through_a_multipart_bound_head() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(&cache, FileId::new(1), "package api; public class Bound {}");
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            concat!(
                "package q; public class Bound { public static class Missing { ",
                "public static int WRONG = 1; } }",
            ),
        );
        publish_java_member_facts(
            &cache,
            FileId::new(3),
            concat!(
                "package app; import api.Bound; import q.*; ",
                "public class Child extends Bound.Missing {}",
            ),
        );

        assert!(
            cache
                .snapshot()
                .resolve_field_signature("app.Child", "WRONG")
                .is_none(),
            "a missing suffix on the explicit Bound binding must not fall through to q.Bound"
        );
    }

    #[test]
    fn non_public_dependency_fields_do_not_cross_the_fallback_boundary() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            concat!(
                "package api; class Base { private static int PRIVATE = 1; ",
                "static int PACKAGE = 2; protected static int PROTECTED = 3; ",
                "public static int PUBLIC = 4; }",
            ),
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            "package other; import api.Base; class Child extends Base {}",
        );
        let snapshot = cache.snapshot();

        for inaccessible in ["PRIVATE", "PACKAGE", "PROTECTED"] {
            assert!(
                snapshot
                    .resolve_field_signature("other.Child", inaccessible)
                    .is_none(),
                "fallback must not invent cross-file access to {inaccessible}"
            );
        }
        assert!(snapshot
            .resolve_field_signature("other.Child", "PUBLIC")
            .is_some());
    }

    #[test]
    fn private_superclass_field_does_not_hide_accessible_interface_field() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            concat!(
                "package api; public class Base { private static int PRIVATE_NAME = 1; ",
                "static int PACKAGE_NAME = 2; }",
            ),
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            concat!(
                "package api; public interface Contract { int PRIVATE_NAME = 3; ",
                "int PACKAGE_NAME = 4; }",
            ),
        );
        publish_java_member_facts(
            &cache,
            FileId::new(3),
            concat!(
                "package other; import api.*; public class Child extends Base ",
                "implements Contract {}",
            ),
        );
        let snapshot = cache.snapshot();

        assert_eq!(
            snapshot
                .resolve_field_signature("other.Child", "PRIVATE_NAME")
                .map(|(owner, _)| owner.to_string()),
            Some("api.Contract".to_owned()),
            "the private superclass declaration is not inherited"
        );
        assert!(
            snapshot
                .resolve_field_signature("other.Child", "PACKAGE_NAME")
                .is_none(),
            "without an exact package-relation fact the fallback must not drop the package field"
        );
    }

    #[test]
    fn same_package_superclass_field_dominates_interface_field() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            "package p; public class Base { static int VALUE = 1; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            "package p; public interface Contract { int VALUE = 2; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(3),
            concat!(
                "package p; public class Child extends Base ",
                "implements Contract {}",
            ),
        );

        assert_eq!(
            cache
                .snapshot()
                .resolve_field_signature_from("p.Child", "VALUE", Some("p"), "p.Child")
                .map(|(owner, _)| owner),
            Some(Arc::from("p.Base")),
            "the class-chain declaration must win before interface lookup"
        );
    }

    #[test]
    fn package_field_is_not_inherited_back_across_a_foreign_package_edge() {
        let cache = MethodSignatureCache::new();
        publish_java_member_facts(
            &cache,
            FileId::new(1),
            "package p; public class Base { static int VALUE = 1; }",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(2),
            "package q; public class Middle extends p.Base {}",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(3),
            "package p; public class Child extends q.Middle {}",
        );
        publish_java_member_facts(
            &cache,
            FileId::new(4),
            "package p; public class DirectChild extends Base {}",
        );
        let snapshot = cache.snapshot();

        assert!(
            snapshot
                .resolve_field_signature_from("p.Child", "VALUE", Some("p"), "p.Child")
                .is_none(),
            "q.Middle cannot inherit p.Base.VALUE, so p.Child cannot acquire it again"
        );
        assert_eq!(
            snapshot
                .resolve_field_signature_from("p.DirectChild", "VALUE", Some("p"), "p.DirectChild",)
                .map(|(owner, _)| owner),
            Some(Arc::from("p.Base")),
            "a direct same-package subclass must retain package-field access"
        );
        assert!(
            snapshot
                .resolve_field_signature_from("p.DirectChild", "VALUE", Some("p"), "p.Client",)
                .is_some(),
            "another class in p may access the inherited package field through DirectChild"
        );
    }

    #[test]
    fn field_facts_replace_atomically_and_preserve_duplicate_ambiguity() {
        let cache = MethodSignatureCache::new();
        let first = FileId::new(1);
        let second = FileId::new(2);
        assert!(cache.update_file_member_facts(
            first,
            Vec::new(),
            vec![(
                "one.Values.COUNT".to_owned(),
                FieldSignature::new(ValueKind::PrimitiveNumeric, true, FieldVisibility::Public,),
            )],
            vec!["one.Values".to_owned()],
            Vec::new(),
        ));
        assert_eq!(
            cache.snapshot().field_signature("one.Values", "COUNT"),
            Some(FieldSignature::new(
                ValueKind::PrimitiveNumeric,
                true,
                FieldVisibility::Public,
            ))
        );
        assert!(cache
            .snapshot()
            .field_signature("Values", "COUNT")
            .is_none());
        assert!(cache.update_file_member_facts(
            second,
            Vec::new(),
            vec![(
                "one.Values.COUNT".to_owned(),
                FieldSignature::new(ValueKind::PrimitiveFloating, false, FieldVisibility::Public,),
            )],
            vec!["one.Values".to_owned()],
            Vec::new(),
        ));
        assert!(cache
            .snapshot()
            .field_signature("one.Values", "COUNT")
            .is_none());
        assert!(cache.remove_file_signatures(second));
        assert_eq!(
            cache.snapshot().field_signature("one.Values", "COUNT"),
            Some(FieldSignature::new(
                ValueKind::PrimitiveNumeric,
                true,
                FieldVisibility::Public,
            ))
        );
    }

    #[test]
    fn method_only_member_change_reuses_the_declaration_fact_root() {
        let cache = MethodSignatureCache::new();
        let file = FileId::new(1);
        let fields = || {
            vec![(
                "api.Values.COUNT".to_owned(),
                FieldSignature::new(ValueKind::PrimitiveNumeric, true, FieldVisibility::Public),
            )]
        };
        let signature = |return_nullability| {
            vec![(
                "api.Values.read()".to_owned(),
                MethodSignature::new(return_nullability, Vec::new(), file),
            )]
        };
        assert!(cache.update_file_member_facts(
            file,
            signature(Nullability::NonNull),
            fields(),
            vec!["api.Values".to_owned()],
            Vec::new(),
        ));
        let before = cache.snapshot();

        assert!(cache.update_file_member_facts(
            file,
            signature(Nullability::Nullable),
            fields(),
            vec!["api.Values".to_owned()],
            Vec::new(),
        ));
        let after = cache.snapshot();

        assert!(Arc::ptr_eq(
            &before.declaration_facts,
            &after.declaration_facts
        ));
    }

    #[test]
    fn nested_lambda_parameters_do_not_change_method_identity() {
        let root = parse_java(
            r#"
            package com.example;
            class Service {
                void apply(String value) {
                    java.util.function.Function<String, String> f = item -> item;
                }
            }
            "#,
        );

        let signatures = extract_method_signatures(&root, FileId::new(1));
        assert_eq!(signatures.len(), 1);
        assert_eq!(signatures[0].0, "com.example.Service.apply(String)");
        assert_eq!(signatures[0].1.param_nullabilities.len(), 1);
    }

    #[test]
    fn test_extract_method_signatures_with_annotations() {
        let source = r#"
package com.example;

import org.jetbrains.annotations.NotNull;
import org.jetbrains.annotations.Nullable;

public class Service {
    @NotNull
    public String getRequired() {
        return "value";
    }

    @Nullable
    public String getOptional() {
        return null;
    }
}
"#;

        let root = parse_java(source);

        let file_id = FileId::new(1);
        let signatures = extract_method_signatures(&root, file_id);

        assert_eq!(signatures.len(), 2);

        let required = signatures
            .iter()
            .find(|(fqn, _)| fqn.contains("getRequired"))
            .unwrap();
        assert_eq!(required.1.return_nullability, Nullability::NonNull);

        let optional = signatures
            .iter()
            .find(|(fqn, _)| fqn.contains("getOptional"))
            .unwrap();
        assert_eq!(optional.1.return_nullability, Nullability::Nullable);
    }

    #[test]
    fn test_extract_method_signatures_overloaded() {
        let source = r#"
package com.example;

public class Calculator {
    public int add(int a, int b) {
        return a + b;
    }

    public double add(double a, double b) {
        return a + b;
    }

    public String add(String a, String b) {
        return a + b;
    }
}
"#;

        let root = parse_java(source);

        let file_id = FileId::new(1);
        let signatures = extract_method_signatures(&root, file_id);

        assert_eq!(signatures.len(), 3);

        // All three overloads should have different FQNs
        let fqns: Vec<&String> = signatures.iter().map(|(fqn, _)| fqn).collect();
        assert!(fqns.contains(&&"com.example.Calculator.add(int,int)".to_string()));
        assert!(fqns.contains(&&"com.example.Calculator.add(double,double)".to_string()));
        assert!(fqns.contains(&&"com.example.Calculator.add(String,String)".to_string()));
    }

    #[test]
    fn test_extract_method_signatures_no_package() {
        let source = r#"
public class DefaultPackage {
    public void test() {
    }
}
"#;

        let root = parse_java(source);

        let file_id = FileId::new(1);
        let signatures = extract_method_signatures(&root, file_id);

        assert_eq!(signatures.len(), 1);
        assert_eq!(signatures[0].0, "DefaultPackage.test()");
    }

    #[test]
    fn test_version_bumping() {
        let cache = MethodSignatureCache::new();
        assert_eq!(cache.version(), 0);

        assert_eq!(cache.bump_version(), 1);
        assert_eq!(cache.version(), 1);

        assert_eq!(cache.bump_version(), 2);
        assert_eq!(cache.version(), 2);
    }

    #[test]
    fn test_extract_method_signatures_array_params() {
        let source = r#"
package com.example;

import javax.annotation.Nullable;

public class Service {
    public void process(
        @Nullable String[] items,
        @Nullable int[] counts,
        int limit,
        @Nullable int[][] matrix,
        @Nullable int[]... samples
    ) {
    }
}
"#;

        let root = parse_java(source);
        let file_id = FileId::new(1);
        let signatures = extract_method_signatures(&root, file_id);

        assert_eq!(signatures.len(), 1);
        let (fqn, signature) = &signatures[0];
        assert_eq!(
            fqn,
            "com.example.Service.process(String[],int[],int,int[][],int[]...)"
        );
        assert_eq!(
            signature.param_nullabilities,
            vec![
                Nullability::Nullable,
                Nullability::Nullable,
                Nullability::NonNull,
                Nullability::Nullable,
                Nullability::Nullable
            ]
        );
    }

    #[test]
    fn primitive_array_returns_are_reference_contracts() {
        let source = r#"
package com.example;

import javax.annotation.Nullable;

public class Service {
    @Nullable public int[] optionalCounts() { return null; }
    public int[] unknownCounts() { return null; }
    public int count() { return 0; }
}
"#;

        let signatures: HashMap<_, _> =
            extract_method_signatures(&parse_java(source), FileId::new(1))
                .into_iter()
                .collect();
        assert_eq!(
            signatures["com.example.Service.optionalCounts()"].return_nullability,
            Nullability::Nullable
        );
        assert_eq!(
            signatures["com.example.Service.unknownCounts()"].return_nullability,
            Nullability::Unknown
        );
        assert_eq!(
            signatures["com.example.Service.count()"].return_nullability,
            Nullability::NonNull
        );
    }

    #[test]
    fn extracts_contracts_from_every_named_type_kind() {
        let source = r#"
package api;

interface Gateway {
    @Nullable String fetch();
}

abstract class Base {
    @Nonnull abstract String require(@Nullable String fallback);
}

enum Mode {
    FAST;
    @Nullable String label() { return null; }
}

record Result(String value) {
    @Nonnull String display() { return value; }
}

@interface Marker {
    @Nonnull String value();
}
"#;

        let signatures = extract_method_signatures(&parse_java(source), FileId::new(1));
        let by_fqn: HashMap<_, _> = signatures.into_iter().collect();

        assert_eq!(
            by_fqn["api.Gateway.fetch()"].return_nullability,
            Nullability::Nullable
        );
        assert_eq!(
            by_fqn["api.Base.require(String)"].param_nullabilities,
            vec![Nullability::Nullable]
        );
        assert_eq!(
            by_fqn["api.Mode.label()"].return_nullability,
            Nullability::Nullable
        );
        assert_eq!(
            by_fqn["api.Result.display()"].return_nullability,
            Nullability::NonNull
        );
        assert_eq!(
            by_fqn["api.Marker.value()"].return_nullability,
            Nullability::NonNull
        );
        assert_eq!(by_fqn.len(), 5);
    }

    #[test]
    fn nested_methods_belong_only_to_their_qualified_type_owner() {
        let source = r#"
package api;

class Outer {
    @Nonnull String own() { return "outer"; }

    class Inner {
        @Nullable String value() { return null; }
    }

    interface Contract {
        @Nonnull String value();
    }
}

class Other {
    class Inner {
        @Nonnull String value() { return "other"; }
    }
}
"#;

        let signatures = extract_method_signatures(&parse_java(source), FileId::new(1));
        let by_fqn: HashMap<_, _> = signatures.into_iter().collect();

        assert!(by_fqn.contains_key("api.Outer.own()"));
        assert_eq!(
            by_fqn["api.Outer.Inner.value()"].return_nullability,
            Nullability::Nullable
        );
        assert_eq!(
            by_fqn["api.Outer.Contract.value()"].return_nullability,
            Nullability::NonNull
        );
        assert_eq!(
            by_fqn["api.Other.Inner.value()"].return_nullability,
            Nullability::NonNull
        );
        assert!(!by_fqn.contains_key("api.Outer.value()"));
        assert!(!by_fqn.contains_key("api.Other.value()"));
        assert!(!by_fqn.contains_key("api.Inner.value()"));
        assert_eq!(by_fqn.len(), 4);
    }
}
