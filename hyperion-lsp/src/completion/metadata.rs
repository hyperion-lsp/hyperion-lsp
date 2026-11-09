// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Completion Item Metadata
//!
//! Cached metadata stored in CompletionItem.data field to avoid recalculating
//! static scores on every completion request.
//!
//! # Performance Impact
//!
//! Before: 200 completions × workspace query = 10-30ms overhead
//! After: 200 completions × JSON deserialize = <1ms overhead
//! **Net gain: 10-30ms per completion** (10-30% faster on large projects)
//!
//! # Usage
//!
//! ```ignore
//! use crate::completion::metadata::CompletionItemMetadata;
//!
//! // During completion item creation:
//! let metadata = CompletionItemMetadata::new(&simple_name)
//!     .with_fqn(format!("{}.{}", package, class))
//!     .with_item_type("String");
//!
//! CompletionItem {
//!     label: simple_name.to_string(),
//!     data: metadata.to_value(),
//!     ..Default::default()
//! }
//!
//! // During ranking:
//! if let Some(meta) = CompletionItemMetadata::from_item(&item) {
//!     let cached_score = meta.alphabetical_score;
//!     let fqn = meta.fqn.unwrap_or_default();
//! }
//! ```

use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::CompletionItem;

/// Cached metadata for completion ranking
///
/// Stored in CompletionItem.data to avoid recalculating static scores.
/// This eliminates expensive workspace index queries on every keystroke.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItemMetadata {
    /// Stable semantic identity used to distinguish overloads and same-named symbols.
    #[serde(default)]
    pub semantic_id: Option<String>,

    /// Fully qualified name for accurate usage lookup
    ///
    /// When present, enables direct FQN-based lookup instead of expensive
    /// simple-name-based search through all symbols.
    pub fqn: Option<String>,

    /// Pre-calculated usage score (0.0 to 1.0)
    ///
    /// If present, avoids workspace index query entirely.
    /// If None but fqn is present, use direct FQN lookup.
    pub usage_score: Option<f64>,

    /// Pre-calculated alphabetical score (0.0 to 1.0)
    ///
    /// Calculated from first character position in alphabet (a=1.0, z=0.0).
    /// Always present since it's computed during metadata creation.
    pub alphabetical_score: f64,

    /// Extracted type for type compatibility scoring
    ///
    /// E.g., "String", "List", "int". Used for type-based ranking
    /// without re-parsing the detail field.
    pub item_type: Option<String>,

    /// Locality/kind group used for ranking; lower ranks first (see
    /// [`rank_group`]). Generators know where an item came from, the ranker
    /// does not, so the fact travels with the item.
    #[serde(default)]
    pub group: Option<u8>,

    /// The type that declares this member, for ranking a receiver's own
    /// members ahead of inherited ones.
    #[serde(default)]
    pub declared_in: Option<String>,

    /// FQN of the type to import when accepting this item, when it differs
    /// from what the label names (a nested type is written through, and
    /// imported as, its outer type).
    #[serde(default)]
    pub import_fqn: Option<String>,

    /// Order among types of the same group (lower first); see
    /// `TypeCandidate::hint`.
    #[serde(default)]
    pub type_hint: Option<u8>,
}

/// Ranking groups. Lower ranks first. Within one match-quality bucket these
/// decide the order the way IntelliJ's relevance does: what is closest to the
/// cursor wins.
pub mod rank_group {
    /// Local variable or parameter.
    pub const LOCAL: u8 = 0;
    /// Field of an enclosing type.
    pub const FIELD: u8 = 1;
    /// Member declared by the receiver's own type.
    pub const OWN_MEMBER: u8 = 2;
    /// Member inherited from a superclass or interface (not `Object`).
    pub const INHERITED_MEMBER: u8 = 3;
    /// Method inherited from `java.lang.Object`.
    pub const OBJECT_MEMBER: u8 = 6;
    /// Type from the workspace, already imported, or in the same package.
    pub const NEAR_TYPE: u8 = 2;
    /// One of the JDK classes nearly every program uses (`ArrayList`, `Optional`, ...).
    pub const POPULAR_TYPE: u8 = 3;
    /// Type from `java.lang`.
    pub const JAVA_LANG_TYPE: u8 = 4;
    /// Type from the commonly used JDK packages (`java.util`, `java.io`, ...).
    pub const COMMON_TYPE: u8 = 5;
    /// Any other public type.
    pub const OTHER_TYPE: u8 = 6;
    /// Nested type reached through its outer type.
    pub const NESTED_TYPE: u8 = 7;
    /// Language keyword.
    pub const KEYWORD: u8 = 4;
    /// Snippet or live template.
    pub const SNIPPET: u8 = 7;
}

impl CompletionItemMetadata {
    /// Create new metadata with alphabetical score calculated from label
    ///
    /// Other fields default to None and can be set via builder methods.
    pub fn new(label: &str) -> Self {
        Self {
            semantic_id: None,
            fqn: None,
            usage_score: None,
            alphabetical_score: Self::calculate_alphabetical_score(label),
            item_type: None,
            group: None,
            declared_in: None,
            import_fqn: None,
            type_hint: None,
        }
    }

    /// Set the ranking group.
    pub const fn with_group(mut self, group: u8) -> Self {
        self.group = Some(group);
        self
    }

    /// Set the type ordering hint.
    pub const fn with_type_hint(mut self, hint: u8) -> Self {
        self.type_hint = Some(hint);
        self
    }

    /// Set the type to import when the item is accepted.
    pub fn with_import_fqn(mut self, fqn: String) -> Self {
        self.import_fqn = Some(fqn);
        self
    }

    /// Set the declaring type of a member.
    pub fn with_declared_in(mut self, owner: String) -> Self {
        self.declared_in = Some(owner);
        self
    }

    /// Set the fully qualified name
    pub fn with_fqn(mut self, fqn: String) -> Self {
        self.semantic_id = Some(fqn.clone());
        self.fqn = Some(fqn);
        self
    }

    /// Set an identity when an FQN is unavailable (for example, a local declaration).
    pub fn with_semantic_id(mut self, semantic_id: String) -> Self {
        self.semantic_id = Some(semantic_id);
        self
    }

    /// Set the pre-calculated usage score
    pub const fn with_usage_score(mut self, score: f64) -> Self {
        self.usage_score = Some(score);
        self
    }

    /// Set the item type
    pub fn with_item_type(mut self, item_type: String) -> Self {
        self.item_type = Some(item_type);
        self
    }

    /// Calculate alphabetical score from label's first character
    ///
    /// Returns score from 0.0 (z) to 1.0 (a), with 0.5 for non-alphabetic.
    /// Used as tiebreaker in completion ranking (10% weight).
    fn calculate_alphabetical_score(label: &str) -> f64 {
        let Some(first_char) = label.chars().next() else {
            return 0.5;
        };
        let first_char = first_char.to_ascii_lowercase();
        if !first_char.is_ascii_alphabetic() {
            return 0.5;
        }
        let position = f64::from(first_char as u8 - b'a');
        1.0 - (position / 26.0)
    }

    /// Serialize to JSON Value for storing in CompletionItem.data
    ///
    /// Returns None if serialization fails (should never happen for valid metadata).
    pub fn to_value(&self) -> Option<serde_json::Value> {
        serde_json::to_value(self).ok()
    }

    /// Deserialize from CompletionItem.data field
    ///
    /// Returns None if:
    /// - item.data is None
    /// - data is not valid CompletionItemMetadata JSON
    /// - deserialization fails
    ///
    /// This is used by the ranker to retrieve cached scores.
    pub fn from_item(item: &CompletionItem) -> Option<Self> {
        item.data
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alphabetical_score() {
        let meta_a = CompletionItemMetadata::new("ArrayList");
        let meta_z = CompletionItemMetadata::new("ZipFile");
        let meta_m = CompletionItemMetadata::new("Map");

        assert!(meta_a.alphabetical_score > meta_z.alphabetical_score);
        assert!(meta_m.alphabetical_score > meta_z.alphabetical_score);
        assert!(meta_a.alphabetical_score > meta_m.alphabetical_score);
    }

    #[test]
    fn test_builder_pattern() {
        let meta = CompletionItemMetadata::new("User")
            .with_fqn("com.example.User".to_string())
            .with_usage_score(0.75)
            .with_item_type("User".to_string());

        assert_eq!(meta.fqn, Some("com.example.User".to_string()));
        assert_eq!(meta.usage_score, Some(0.75));
        assert_eq!(meta.item_type, Some("User".to_string()));
    }

    #[test]
    // Scores are serialized directly, so this roundtrip intentionally requires bit-exact equality.
    #[allow(clippy::float_cmp)]
    fn test_serialization_roundtrip() {
        let original = CompletionItemMetadata::new("Test")
            .with_fqn("com.example.Test".to_string())
            .with_usage_score(0.5);

        let value = original.to_value().expect("Serialization failed");
        let item = CompletionItem {
            label: "Test".to_string(),
            data: Some(value),
            ..Default::default()
        };

        let deserialized =
            CompletionItemMetadata::from_item(&item).expect("Deserialization failed");

        assert_eq!(deserialized.fqn, original.fqn);
        assert_eq!(deserialized.usage_score, original.usage_score);
        assert_eq!(deserialized.alphabetical_score, original.alphabetical_score);
    }
}
