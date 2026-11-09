// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Method chain parsing and qualified access detection
//!
//! ## Submodules
//!
//! - `segment_parser` - Chain parsing from syntax tokens
//! - `qualified_access` - Qualified member access detection (obj.member)
//! - `extraction` - Receiver and identifier extraction helpers

mod extraction;
mod qualified_access;
mod segment_parser;

pub use extraction::{extract_first_identifier, extract_receiver, extract_receiver_text};
pub use qualified_access::detect_qualified_access;
pub use segment_parser::parse_method_chain;

/// Represents a segment in a method chain
#[derive(Debug, Clone)]
pub struct ChainSegment {
    pub name: String,
    pub is_method_call: bool,
    /// Type from cast expression: ((Type) expr) -> Some("Type")
    pub cast_type: Option<String>,
    /// Exact call expression when this segment came from the structured Java AST.
    /// Text-recovery fallbacks leave this absent and must not claim typed resolution.
    pub call_range: Option<rowan::TextRange>,
}
