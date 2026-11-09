// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Generic type parsing and substitution
//!
//! This module handles parsing type strings from WorkspaceIndex into Type enums,
//! including support for:
//! - Primitive types (int, boolean, etc.)
//! - Reference types (String, User, etc.)
//! - Fully qualified names (java.util.List)
//! - Generic types (List<String>, Map<K, V>)
//! - Nested generics (Map<String, List<Integer>>)
//! - Array types (String[], int[][])
//! - Diamond operator (List<>)

use super::TypeInferenceEngine;
use crate::symbol::Type;
use crate::type_parser;

impl TypeInferenceEngine {
    /// Parse a type string from WorkspaceIndex into a Type.
    ///
    /// Thin facade over [`type_parser::parse_type_string`]. The instance method
    /// shape exists because most callers already hold a `&TypeInferenceEngine`;
    /// new code can call the canonical free function directly.
    #[must_use]
    pub fn parse_type_string(&self, type_str: &str) -> Option<Type> {
        type_parser::parse_type_string(type_str)
    }
}
