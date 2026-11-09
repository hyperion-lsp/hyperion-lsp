// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Error types for the formatter

use thiserror::Error;

/// Errors that can occur during formatting
#[derive(Debug, Error)]
pub enum FormatError {
    /// Failed to parse the input source code
    #[error("parse error: {0}")]
    ParseError(String),

    /// The specified range is invalid
    #[error("invalid range: {start}..{end} (source length: {source_len})")]
    InvalidRange {
        start: usize,
        end: usize,
        source_len: usize,
    },

    /// Internal formatter error (should not occur in normal usage)
    #[error("internal error: {0}")]
    Internal(String),
}
