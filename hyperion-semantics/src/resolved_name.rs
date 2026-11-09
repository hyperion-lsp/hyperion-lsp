// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Unified name resolution result type.
//!
//! `ResolvedName` is the shared contract between the semantic model and all LSP features.
//! Every identifier in the AST maps to one of these variants after resolution. Navigation,
//! hover, diagnostics, completion, and references all consume this type.
//!
//! This replaces the per-feature resolution that previously caused divergence — where
//! goto-definition could resolve a symbol but diagnostics reported it as undefined.

use crate::symbol::{ScopeId, SourceLocation, Type};

/// An analyzed source spelling that cannot denote a canonical symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRejection {
    pub kind: NameRejectionKind,
    pub receiver_fqn: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRejectionKind {
    Inaccessible,
    Ambiguous,
    Missing,
}

/// What a name resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedName {
    /// A class, interface, enum, record, or annotation type
    Type {
        fqn: String,
        /// None for external types (JDK not indexed)
        location: Option<SourceLocation>,
    },
    /// A method (instance or static)
    Method {
        owner_fqn: String,
        name: String,
        return_type: Type,
        location: Option<SourceLocation>,
        /// Modifier prefix for hover display (e.g., "static ", "public static ")
        modifier_prefix: String,
    },
    /// A field (instance or static)
    Field {
        owner_fqn: String,
        name: String,
        field_type: Type,
        location: Option<SourceLocation>,
        /// Modifier prefix for hover display (e.g., "static final ", "public ")
        modifier_prefix: String,
    },
    /// An enum constant
    EnumConstant {
        enum_fqn: String,
        name: String,
        location: Option<SourceLocation>,
    },
    /// A local variable or parameter
    LocalVariable {
        name: String,
        var_type: Type,
        scope: ScopeId,
        location: Option<SourceLocation>,
        is_parameter: bool,
    },
    /// A type parameter (T, E, K, V, etc.)
    TypeParameter { name: String, bounds: Vec<Type> },
    /// A package reference
    Package { fqn: String },
    /// External/unindexed — symbol assumed to exist, suppress diagnostics
    External { fqn: String },
    /// An authoritative negative resolution. Feature layers must not retry
    /// descriptorless or simple-name fallbacks for this source range.
    Rejected(NameRejection),
}

impl ResolvedName {
    /// Returns true if this resolution has a navigable source location.
    pub const fn location(&self) -> Option<&SourceLocation> {
        match self {
            Self::Type { location, .. }
            | Self::Method { location, .. }
            | Self::Field { location, .. }
            | Self::EnumConstant { location, .. }
            | Self::LocalVariable { location, .. } => location.as_ref(),
            Self::TypeParameter { .. }
            | Self::Package { .. }
            | Self::External { .. }
            | Self::Rejected(_) => None,
        }
    }

    /// Returns the FQN if this resolution carries one.
    pub fn fqn(&self) -> Option<&str> {
        match self {
            Self::Type { fqn, .. } | Self::Package { fqn } | Self::External { fqn } => Some(fqn),
            Self::Method { owner_fqn, .. } | Self::Field { owner_fqn, .. } => Some(owner_fqn),
            Self::EnumConstant { enum_fqn, .. } => Some(enum_fqn),
            Self::LocalVariable { .. } | Self::TypeParameter { .. } | Self::Rejected(_) => None,
        }
    }
}
