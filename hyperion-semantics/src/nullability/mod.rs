// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Nullability tracking for Java types
//!
//! Implements a dataflow-based nullability analysis inspired by TypeScript's
//! control flow analysis. Tracks nullability through code paths with narrowing.

mod access_path;
pub mod analysis;
mod annotations;
pub mod cfg;
pub mod environment;
pub mod extraction;
pub mod package_defaults;
pub mod patterns;
pub mod signature_cache;
pub(crate) mod source_methods;

pub use access_path::stable_access_path;
#[allow(deprecated)]
pub use analysis::{
    accepted_return_nullability, analyze, collect_call_site_nullability, expression_nullability,
    infer_return_nullability, method_call_descriptor, DiagnosticKind, MethodCallKey, MethodContext,
    NullabilityAnalysis, NullabilityDiagnostic, NullabilityResolver, SourceLocation,
};
pub(crate) use annotations::annotation_name;
pub use annotations::{
    has_nonnull_annotation, has_nullable_annotation, is_package_nonnull_annotation,
    NONNULL_ANNOTATIONS, NULLABLE_ANNOTATIONS, PACKAGE_NONNULL_ANNOTATIONS,
};
pub use cfg::{
    BasicBlock, BlockId, ControlFlowGraph, ExceptionTypeRevision, ExprRef, StatementRef, Terminator,
};
pub use environment::{NullabilityEnv, VariableId};
pub use extraction::{
    collect_class_method_returns, collect_local_variables, extract_explicit_nonnull_parameters,
    extract_method_info, extract_parameter_nullability, find_enclosing_class_name,
    qualified_type_name, MethodNullabilityInfo,
};
pub use package_defaults::{
    extract_package_defaults, extract_package_name, PackageDefaultsRegistry,
    PackageNullabilityDefaults,
};
pub use patterns::{analyze_condition, NullCheckCondition, SimpleResolver, VariableResolver};
pub use signature_cache::{
    extract_declared_type_names, extract_field_signatures, extract_method_signatures,
    extract_type_hierarchy_facts, FieldSignature, MethodSignature, MethodSignatureCache,
    MethodSignatureSnapshot, TypeHierarchyFact, WorkspaceFactRevision, WorkspaceMemberFactRevision,
};

use crate::symbol::Type;
use std::collections::HashMap;

/// Nullability analysis results for an entire file
///
/// This type wraps per-method analysis results and file-level diagnostics
/// for caching in Salsa queries. The analysis is computed lazily and only
/// recomputed when the file's parse tree changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNullabilityAnalysis {
    /// Analysis results per method (keyed by method byte offset)
    pub methods: HashMap<u32, NullabilityAnalysis>,
    /// File-level diagnostics
    pub diagnostics: Vec<NullabilityDiagnostic>,
}

impl FileNullabilityAnalysis {
    /// Creates an empty analysis result (for non-Java files or files without executable code)
    pub fn empty() -> Self {
        Self {
            methods: HashMap::new(),
            diagnostics: Vec::new(),
        }
    }
}

/// Nullability lattice for dataflow analysis
///
/// Partial order: Bottom < {NonNull, Null} < Nullable < Unknown
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Nullability {
    /// Provably non-null (after null check, constructor result, primitives)
    NonNull,

    /// Provably null (after `x == null` branch)
    Null,

    /// May be null (annotated @Nullable, or after join of null/non-null)
    Nullable,

    /// Unknown nullability (unannotated platform types)
    #[default]
    Unknown,

    /// Unreachable code path (after return, throw, infinite loop)
    Bottom,
}

impl Nullability {
    /// Join two nullability states at control flow merge points
    pub const fn join(self, other: Self) -> Self {
        match (self, other) {
            // Bottom is identity for join
            (Self::Bottom, x) | (x, Self::Bottom) => x,
            // Same state joins to itself
            (Self::NonNull, Self::NonNull) => Self::NonNull,
            (Self::Null, Self::Null) => Self::Null,
            // Unknown absorbs everything except Bottom
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            // Nullable absorbs NonNull, Null, and NonNull+Null mixed
            _ => Self::Nullable,
        }
    }

    /// Can this value be dereferenced without null check?
    pub const fn is_safe_to_dereference(self) -> bool {
        matches!(self, Self::NonNull)
    }

    /// Is this provably null?
    pub const fn is_definitely_null(self) -> bool {
        matches!(self, Self::Null)
    }

    /// Is this potentially null (needs null check before dereference)?
    pub const fn may_be_null(self) -> bool {
        matches!(self, Self::Null | Self::Nullable | Self::Unknown)
    }

    /// Is this unreachable code?
    pub const fn is_bottom(self) -> bool {
        matches!(self, Self::Bottom)
    }

    /// Join two nullability states for return value analysis.
    ///
    /// Unlike the full `join()` method which handles control flow merge points
    /// with Bottom as identity, this is simplified for joining return values:
    /// - NonNull + NonNull = NonNull
    /// - Anything + Nullable = Nullable
    /// - Otherwise = Unknown
    ///
    /// This is used for lambda return type inference where we need to combine
    /// multiple possible return paths (e.g., try-catch with different returns).
    pub const fn join_single(self, other: Self) -> Self {
        match (self, other) {
            (Self::NonNull, Self::NonNull) => Self::NonNull,
            (Self::Nullable | Self::Null, _) | (_, Self::Nullable | Self::Null) => Self::Nullable,
            _ => Self::Unknown,
        }
    }
}

/// A type with nullability tracking
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NullableType {
    /// The underlying type (String, User, etc)
    pub base_type: Type,

    /// Nullability state
    pub nullability: Nullability,
}

impl NullableType {
    /// Create a non-null type
    pub const fn non_null(base_type: Type) -> Self {
        Self {
            base_type,
            nullability: Nullability::NonNull,
        }
    }

    /// Create a nullable type
    pub const fn nullable(base_type: Type) -> Self {
        Self {
            base_type,
            nullability: Nullability::Nullable,
        }
    }

    /// Create an unknown nullability type (platform type)
    pub const fn unknown(base_type: Type) -> Self {
        Self {
            base_type,
            nullability: Nullability::Unknown,
        }
    }

    /// Create from type with annotation-based nullability
    pub fn from_type_with_annotations(base_type: Type, annotations: &[String]) -> Self {
        let nullability = if base_type.is_primitive() {
            Nullability::NonNull
        } else if has_nonnull_annotation(annotations) {
            Nullability::NonNull
        } else if has_nullable_annotation(annotations) {
            Nullability::Nullable
        } else {
            Nullability::Unknown
        };

        Self {
            base_type,
            nullability,
        }
    }

    /// Join with another nullable type (for ternary, phi nodes)
    pub fn join(&self, other: &Self) -> Self {
        // Types must be compatible - use LUB of base types
        // For now, just use self's base type (caller should ensure compatibility)
        Self {
            base_type: self.base_type.clone(),
            nullability: self.nullability.join(other.nullability),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_same_states() {
        assert_eq!(
            Nullability::NonNull.join(Nullability::NonNull),
            Nullability::NonNull
        );
        assert_eq!(Nullability::Null.join(Nullability::Null), Nullability::Null);
        assert_eq!(
            Nullability::Nullable.join(Nullability::Nullable),
            Nullability::Nullable
        );
        assert_eq!(
            Nullability::Unknown.join(Nullability::Unknown),
            Nullability::Unknown
        );
    }

    #[test]
    fn test_join_nonnull_and_null() {
        assert_eq!(
            Nullability::NonNull.join(Nullability::Null),
            Nullability::Nullable
        );
        assert_eq!(
            Nullability::Null.join(Nullability::NonNull),
            Nullability::Nullable
        );
    }

    #[test]
    fn test_join_with_bottom() {
        assert_eq!(
            Nullability::Bottom.join(Nullability::NonNull),
            Nullability::NonNull
        );
        assert_eq!(
            Nullability::NonNull.join(Nullability::Bottom),
            Nullability::NonNull
        );
        assert_eq!(
            Nullability::Bottom.join(Nullability::Nullable),
            Nullability::Nullable
        );
    }

    #[test]
    fn test_join_with_unknown() {
        assert_eq!(
            Nullability::Unknown.join(Nullability::NonNull),
            Nullability::Unknown
        );
        assert_eq!(
            Nullability::NonNull.join(Nullability::Unknown),
            Nullability::Unknown
        );
        assert_eq!(
            Nullability::Unknown.join(Nullability::Nullable),
            Nullability::Unknown
        );
    }

    #[test]
    fn test_is_safe_to_dereference() {
        assert!(Nullability::NonNull.is_safe_to_dereference());
        assert!(!Nullability::Null.is_safe_to_dereference());
        assert!(!Nullability::Nullable.is_safe_to_dereference());
        assert!(!Nullability::Unknown.is_safe_to_dereference());
        assert!(!Nullability::Bottom.is_safe_to_dereference());
    }

    #[test]
    fn test_may_be_null() {
        assert!(!Nullability::NonNull.may_be_null());
        assert!(Nullability::Null.may_be_null());
        assert!(Nullability::Nullable.may_be_null());
        assert!(Nullability::Unknown.may_be_null());
        assert!(!Nullability::Bottom.may_be_null());
    }

    #[test]
    fn test_default_is_unknown() {
        let default_nullability = Nullability::default();
        assert_eq!(default_nullability, Nullability::Unknown);
    }

    #[test]
    fn test_join_single_same_states() {
        // NonNull + NonNull = NonNull
        assert_eq!(
            Nullability::NonNull.join_single(Nullability::NonNull),
            Nullability::NonNull
        );
        // Nullable + Nullable = Nullable
        assert_eq!(
            Nullability::Nullable.join_single(Nullability::Nullable),
            Nullability::Nullable
        );
    }

    #[test]
    fn test_join_single_nonnull_and_null() {
        // NonNull + Null = Nullable (the main fix case)
        assert_eq!(
            Nullability::NonNull.join_single(Nullability::Null),
            Nullability::Nullable
        );
        // Null + NonNull = Nullable
        assert_eq!(
            Nullability::Null.join_single(Nullability::NonNull),
            Nullability::Nullable
        );
    }

    #[test]
    fn test_join_single_nullable_absorbs() {
        // Nullable + anything = Nullable
        assert_eq!(
            Nullability::Nullable.join_single(Nullability::NonNull),
            Nullability::Nullable
        );
        assert_eq!(
            Nullability::NonNull.join_single(Nullability::Nullable),
            Nullability::Nullable
        );
        assert_eq!(
            Nullability::Nullable.join_single(Nullability::Null),
            Nullability::Nullable
        );
    }

    #[test]
    fn test_join_single_with_unknown() {
        // Unknown + anything non-null-related = Unknown
        assert_eq!(
            Nullability::Unknown.join_single(Nullability::Unknown),
            Nullability::Unknown
        );
        assert_eq!(
            Nullability::NonNull.join_single(Nullability::Unknown),
            Nullability::Unknown
        );
    }
}
