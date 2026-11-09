// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

/// Utility functions for working with tokens and syntax
use crate::syntax::{JavaLanguage, SyntaxKind};
use rowan::SyntaxToken;

/// Check if a token is an identifier (including contextual keywords)
///
/// # Deprecated
/// Use `token.kind().is_identifier()` instead. This function is deprecated in favor
/// of the inherent method on `SyntaxKind` for better API design and const evaluation.
///
/// Contextual keywords like 'record', 'var', 'yield' are only keywords in specific contexts
/// and should be allowed as identifiers elsewhere (parameter names, variable names, field names, etc.)
///
/// # Arguments
/// * `token` - The syntax token to check
///
/// # Returns
/// `true` if the token is an IDENT or a contextual keyword that can act as an identifier
#[deprecated(since = "0.1.0", note = "Use `token.kind().is_identifier()` instead")]
#[must_use]
pub fn is_identifier_token(token: &SyntaxToken<JavaLanguage>) -> bool {
    token.kind().is_identifier()
}

/// Check if a `SyntaxKind` represents an identifier (including contextual keywords)
///
/// # Deprecated
/// Use `kind.is_identifier()` instead. This function is deprecated in favor
/// of the inherent method on `SyntaxKind` for better API design and const evaluation.
///
/// This is a convenience function for cases where you have a `SyntaxKind` instead of a token.
///
/// # Arguments
/// * `kind` - The syntax kind to check
///
/// # Returns
/// `true` if the kind is IDENT or a contextual keyword
#[deprecated(since = "0.1.0", note = "Use `kind.is_identifier()` instead")]
#[must_use]
pub const fn is_identifier_kind(kind: SyntaxKind) -> bool {
    kind.is_identifier()
}
