// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// hyperion-workspace: Cross-file navigation and workspace indexing
//
// This crate provides workspace-wide symbol indexing for Hyperion LSP,
// enabling cross-file go-to-definition, find-references, and workspace symbol search.

mod archive_snapshot;
pub mod model;
pub mod name_match;
pub use model::{
    is_type_declaration_kind, DeclarationNamespace, FileId, FileSource, GenericSupertype,
    KotlinParameterMetadata, KotlinPropertyMetadata, KotlinSymbolMetadata, KotlinTypeAlias,
    KotlinTypeAliasArguments, Reference, ReferenceKind, ResolvedDirectSupertypes,
    ResolvedTypeSupertypes, Symbol, SymbolLocation, SymbolTarget, Throws, TypeGenericSupertypes,
    TypeNameResolution,
};
pub mod cache;
pub mod enum_support;
pub mod groovy_usage_extractor;
pub mod import_resolver;
pub mod index;
pub mod integration_tests;
pub mod jar_indexer;
pub mod javadoc_parser;
pub mod jdk_integration;
pub mod jmod_indexer;
pub mod kotlin_usage_extractor;
pub mod language;
pub mod lombok;
pub mod parent_interface_extractor;
pub mod path_identity;
pub mod position_utils;
pub mod query;
pub mod record_support;
mod refactoring_resources;
pub mod scala_usage_extractor;
pub mod scanner;
mod source_file;
pub mod symbol_extractor;
pub mod type_declaration_extractor;
mod type_name_resolution;
pub mod usage_extractor;
mod usage_resolution;
pub mod usage_stats;

// Re-export main types for convenience
pub use archive_snapshot::{ArchiveContentIdentity, ArchiveFileVersion, ArchiveSnapshot};
pub use cache::{
    cached_to_symbol, symbol_to_cached, CachedFile, CachedIndex, CachedSymbol, IndexCache,
};
pub use hyperion_types::{
    DeclarationImports as ImportMap, DeclarationModifiers as Modifiers, StaticImport, TextRange,
    TypeParameterInfo,
};
pub use index::{
    java_parameter_parts_from_declaration, java_parameter_type_from_declaration,
    CurrentFileDependencies, CurrentUsageLocations, CurrentUsageQueryError,
    DefinitionSourceAuthority, ExclusiveIndexWrites, ExternalArchiveSnapshot,
    ExternalIndexDataBySource, ExternalSourceArchive, ExternalSourceIdentity,
    ExternalSourcePublicationGuard, ExternalSourceRefresh, ExternalSourceRefreshTicket,
    ExternalSourceRevisionToken, FieldAccessQualifier, FileRevisionRemovalOutcome,
    FileRevisionToken, FunctionalInterfaceQuery, FunctionalInterfaceSignature, IndexStats,
    InheritedMethod, PreparedExternalArchivePublication, ProjectedCallableParameter,
    ProjectedCallableSignature, ProjectedEffectiveField, ProjectedEffectiveMethod,
    ProjectedFieldAccessResolution, ProjectedOwnerBinding, ProjectedVariadicStyle,
    RegisteredFilePath, ResolvedDefinition, RestoredFileRevision, RevisionedFileLocations,
    RevisionedLocation, RevisionedLocations, RevisionedResolvedMethodUsages, SourceRevisionOrigin,
    SourceScope, SourceScopeInput, WorkspaceIndex, WorkspaceRevision,
};
pub use jar_indexer::{extract_jar_symbols, extract_jar_symbols_with_docs, JarIndexStats};
pub use javadoc_parser::{parse_javadoc_jar, JavadocExtraction};
pub use language::{
    implicit_single_type_import, implicit_type_import_packages, kotlin_builtin_jvm_fqn, Language,
};
pub use path_identity::{path_lookup_key, PathLookupKey};
pub use query::{CurrentReferences, QueryEngine};
pub use refactoring_resources::RefactoringResourceProof;
pub use source_file::{
    read_source_file, read_source_file_async, read_utf8_file_bounded, MAX_SOURCE_FILE_BYTES,
};
pub use usage_resolution::InvocationArgumentType;
pub use usage_stats::UsageStatistics;

use lsp_types::{Position, Range, SymbolKind};
use std::sync::Arc;

/// Source text paired with the exact workspace-index revision that produced
/// stored byte ranges.
///
/// Unlike a raw filesystem read, this value is safe to use only with ranges
/// captured alongside the same [`FileRevisionToken`].
#[derive(Debug, Clone)]
pub struct IndexedContent {
    /// The indexed source text (Arc to avoid cloning large strings).
    pub content: Arc<String>,
    /// Hash of the exact indexed revision represented by `content`.
    pub content_hash: u64,
    /// Identity of the exact publication that owns these bytes and ranges.
    pub revision: FileRevisionToken,
    /// Path owned by `file_id` when this revision was read.
    pub file_path: std::path::PathBuf,
}

/// Exact current indexed content for a compatibility FileId lookup.
///
/// Disk drift and evicted caller-owned buffers fail closed. New range consumers
/// should use revisioned query APIs and `get_indexed_revision_content`.
#[derive(Debug, Clone)]
pub struct ValidatedContent {
    /// Exact indexed content (Arc to avoid cloning large strings)
    pub content: Arc<String>,
    /// Display path owned by the indexed revision
    pub file_path: std::path::PathBuf,
}

/// Batched canonical hierarchy facts for every nominal declaration owned by
/// one proven file and workspace revision. Both tokens must still be current
/// before lazily publishing these facts.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ResolvedFileSupertypes {
    pub types: Vec<ResolvedTypeSupertypes>,
    pub fingerprint: u64,
    pub file_revision: FileRevisionToken,
    pub workspace_revision: WorkspaceRevision,
}

/// Convert byte offset to LSP Position
pub fn offset_to_position(source: &str, offset: u32) -> Position {
    position_utils::offset_to_position(source, offset as usize)
}

/// Convert TextRange to LSP Range
pub fn text_range_to_range(source: &str, range: TextRange) -> Range {
    let lines = position_utils::LineIndex::new(source);
    Range::new(
        lines.offset_to_position(source, range.start as usize),
        lines.offset_to_position(source, range.end as usize),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn symbol_location_provenance_costs_exactly_one_word() {
        const BEFORE_COMPACT_PROVENANCE: usize = 80;
        const AFTER_COMPACT_PROVENANCE: usize = 88;
        assert_eq!(
            std::mem::size_of::<SymbolLocation>(),
            AFTER_COMPACT_PROVENANCE
        );
        assert_eq!(
            AFTER_COMPACT_PROVENANCE - BEFORE_COMPACT_PROVENANCE,
            std::mem::size_of::<Option<NonZeroU64>>()
        );
    }

    #[test]
    fn test_text_range() {
        let range = TextRange::new(10, 20);
        assert_eq!(range.len(), 10);
        assert!(!range.is_empty());
        assert!(range.contains(15));
        assert!(!range.contains(5));
        assert!(!range.contains(25));
    }

    #[test]
    fn invalid_text_range_length_does_not_underflow() {
        let range = TextRange { start: 20, end: 10 };
        assert_eq!(range.len(), 0);
    }

    #[test]
    fn test_offset_to_position() {
        let source = "package com.test;\n\nimport java.util.List;\n";

        // Start of file
        assert_eq!(
            offset_to_position(source, 0),
            Position {
                line: 0,
                character: 0
            }
        );

        // After first line
        assert_eq!(
            offset_to_position(source, 18),
            Position {
                line: 1,
                character: 0
            }
        );

        // Third line, "List"
        assert_eq!(
            offset_to_position(source, 38),
            Position {
                line: 2,
                character: 19
            }
        );
    }

    #[test]
    fn workspace_offsets_clamp_to_utf8_and_crlf_boundaries() {
        let source = "é😀\r\n界";
        for (offset, expected) in [
            (1, Position::new(0, 0)),
            (3, Position::new(0, 1)),
            (7, Position::new(0, 3)),
            (8, Position::new(1, 0)),
            (u32::MAX, Position::new(1, 1)),
        ] {
            assert_eq!(offset_to_position(source, offset), expected);
        }
        assert_eq!(offset_to_position("", u32::MAX), Position::new(0, 0));
    }

    #[test]
    fn test_import_map() {
        let mut map = ImportMap::new();
        map.current_package = Some("com.acme.utils".into());
        map.add_import("List".to_string(), "java.util.List".to_string());
        map.add_wildcard("java.util".to_string());
        map.add_static_import("max".to_string(), "java.lang.Math.max".to_string());

        assert_eq!(
            map.explicit.get("List"),
            Some(&"java.util.List".to_string())
        );
        assert!(map.wildcards.contains(&"java.util".into()));
        assert_eq!(
            map.static_imports_named("max").next(),
            Some("java.lang.Math.max")
        );
    }
}
