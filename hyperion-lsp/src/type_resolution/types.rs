// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type resolution data structures
//!
//! This module defines the core types used for type resolution in hover and navigation.

use hyperion_workspace::FileId;
use tower_lsp::lsp_types::SymbolKind;

/// How much semantic evidence backs a member-shaped [`TypeInfo`].
///
/// A name-only recovery is useful for hover on incomplete or inapplicable
/// calls. An unavailable projection is different: the index found the member,
/// but conflicting/malformed generic paths make its receiver-relative
/// signature unknowable, so consumers must not recover its raw declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberResolutionStatus {
    Applicable,
    NameOnly,
    ProjectionUnavailable,
}

/// Information about a resolved type
#[derive(Debug, Clone)]
pub struct TypeInfo {
    /// Simple type name (e.g., "Stream")
    pub simple_name: String,

    /// Fully qualified name (e.g., "java.util.stream.Stream")
    pub fqn: String,

    /// Package name (e.g., Some("java.util.stream"))
    pub package: Option<String>,

    /// Symbol kind (CLASS, INTERFACE, ENUM, etc.)
    pub kind: SymbolKind,

    /// Generic type arguments (e.g., ["String"] for Stream<String>).
    /// These are carried through chain resolution for receiver-relative member
    /// projection and must not be discarded between segments.
    pub type_arguments: Vec<String>,

    /// JavaDoc or documentation
    pub documentation: Option<String>,

    /// Method or field return type (for external symbols)
    pub return_type: Option<String>,

    /// File where this type is defined
    pub file_id: Option<FileId>,

    /// Container name (e.g., class name for methods)
    pub container_name: Option<String>,

    /// Method parameters
    pub parameters: Vec<String>,

    /// Present only when this value describes a resolved member rather than a
    /// nominal/primitive type.
    pub member_resolution: Option<MemberResolutionStatus>,
}

/// Errors that can occur during type resolution
#[derive(Debug, Clone)]
pub enum ResolveError {
    /// Symbol not found in scope
    SymbolNotFound(String),

    /// Type could not be resolved via imports
    TypeNotFound(String),

    /// Method not found on type
    MethodNotFound {
        type_fqn: String,
        method_name: String,
    },

    /// Field not found on type
    FieldNotFound {
        type_fqn: String,
        field_name: String,
    },

    /// Ambiguous resolution (multiple candidates)
    Ambiguous { candidates: Vec<String> },

    /// Maximum chain depth exceeded
    ChainTooDeep,

    /// Parse error
    ParseError(String),

    /// Database lock error
    DatabaseError(String),

    /// Other error with message
    Other(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SymbolNotFound(name) => write!(f, "Symbol not found: {name}"),
            Self::TypeNotFound(name) => write!(f, "Type not found: {name}"),
            Self::MethodNotFound {
                type_fqn,
                method_name,
            } => write!(f, "Method '{method_name}' not found on type '{type_fqn}'"),
            Self::FieldNotFound {
                type_fqn,
                field_name,
            } => write!(f, "Field '{field_name}' not found on type '{type_fqn}'"),
            Self::Ambiguous { candidates } => {
                write!(
                    f,
                    "Ambiguous reference with {} candidates",
                    candidates.len()
                )
            }
            Self::ChainTooDeep => write!(f, "Chain resolution depth exceeded"),
            Self::ParseError(msg) => write!(f, "Parse error: {msg}"),
            Self::DatabaseError(msg) => write!(f, "Database error: {msg}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Result type for resolution operations
pub type ResolveResult<T> = Result<T, ResolveError>;
