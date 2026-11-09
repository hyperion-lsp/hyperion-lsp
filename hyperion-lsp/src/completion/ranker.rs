// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Completion Ranking
//!
//! Ranks completion items by relevance using multiple scoring factors:
//! - Usage frequency (40%): How often the symbol is used
//! - Type compatibility (30%): Match with expected type
//! - Pattern recognition (20%): Getter/setter, builder patterns
//! - Alphabetical (10%): Tiebreaker
//!
//! # Example
//! ```ignore
//! let ranker = CompletionRanker::new(workspace_index);
//! let context = RankingContext::new(position, source_text, expected_type);
//! ranker.rank(&mut completions, &context);
//! // Completions are now sorted by relevance, not alphabetically
//! ```

use crate::completion::metadata::{rank_group, CompletionItemMetadata};
use hyperion_workspace::name_match::{match_name, NameMatch};
use hyperion_workspace::WorkspaceIndex;
use std::collections::HashMap;
use std::sync::Arc;
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, CompletionItemTag};
use tracing::debug;

/// The identifier being typed at the end of `text`.
fn trailing_identifier(text: &str) -> &str {
    let start = text
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
        .map_or(0, |(i, c)| i + c.len_utf8());
    &text[start..]
}

/// Group for items whose generator did not classify them.
const fn default_group(item: &CompletionItem) -> u8 {
    match item.kind {
        Some(CompletionItemKind::VARIABLE) => rank_group::LOCAL,
        Some(CompletionItemKind::FIELD | CompletionItemKind::PROPERTY) => rank_group::FIELD,
        Some(CompletionItemKind::METHOD | CompletionItemKind::FUNCTION) => rank_group::OWN_MEMBER,
        Some(CompletionItemKind::KEYWORD) => rank_group::KEYWORD,
        Some(CompletionItemKind::SNIPPET) => rank_group::SNIPPET,
        Some(
            CompletionItemKind::CLASS
            | CompletionItemKind::INTERFACE
            | CompletionItemKind::ENUM
            | CompletionItemKind::CONSTRUCTOR,
        ) => rank_group::OTHER_TYPE,
        _ => rank_group::SNIPPET,
    }
}

/// Ranks completion items by relevance
pub struct CompletionRanker {
    workspace_index: Arc<WorkspaceIndex>,
}

/// Context information for ranking completions
#[derive(Debug, Clone)]
pub struct RankingContext {
    /// Text before the cursor (for pattern detection)
    pub prefix_text: String,

    /// Expected type at cursor position (if known)
    pub expected_type: Option<String>,

    /// Whether we're in a type position (variable declaration, parameter, etc.)
    pub is_type_position: bool,

    /// Previous expression before '.' for member access
    pub receiver_expression: Option<String>,

    /// Type of the receiver expression for member access
    pub receiver_type: Option<String>,
}

impl RankingContext {
    /// Create a new ranking context
    pub const fn new(prefix_text: String) -> Self {
        Self {
            prefix_text,
            expected_type: None,
            is_type_position: false,
            receiver_expression: None,
            receiver_type: None,
        }
    }

    /// Set the expected type at this position
    pub fn with_expected_type(mut self, expected_type: String) -> Self {
        self.expected_type = Some(expected_type);
        self
    }

    /// Mark this as a type position
    pub const fn in_type_position(mut self) -> Self {
        self.is_type_position = true;
        self
    }

    /// Set the receiver expression for member access
    pub fn with_receiver(mut self, receiver: String) -> Self {
        self.receiver_expression = Some(receiver);
        self
    }

    /// Set the receiver type for member access
    pub fn with_receiver_type(mut self, receiver_type: String) -> Self {
        self.receiver_type = Some(receiver_type);
        self
    }
}

impl CompletionRanker {
    /// Create a new completion ranker
    pub const fn new(workspace_index: Arc<WorkspaceIndex>) -> Self {
        Self { workspace_index }
    }

    /// Rank completion items by relevance
    ///
    /// This modifies the `sort_text` field of each completion item to encode
    /// the ranking score. Items with lower sort_text appear first.
    ///
    /// # Arguments
    /// * `items` - Completion items to rank (modified in place)
    /// * `context` - Ranking context (position, expected type, etc.)
    pub fn rank(&self, items: &mut [CompletionItem], context: &RankingContext) {
        debug!("Ranking {} completion items", items.len());

        // Resolve every usage score as one batch. On large workspaces an
        // individual descriptor-less lookup may scan the whole usage table to
        // aggregate overloads; doing that once per item dominated completion.
        let usage_candidates: Vec<Vec<String>> = items
            .iter()
            .map(|item| {
                CompletionItemMetadata::from_item(item)
                    .and_then(|metadata| metadata.fqn.map(|fqn| vec![fqn]))
                    .unwrap_or_else(|| self.workspace_index.get_fqns_by_simple_name(&item.label))
            })
            .collect();
        let mut unique_fqns = Vec::new();
        let mut positions = HashMap::new();
        for fqn in usage_candidates.iter().flatten() {
            if !positions.contains_key(fqn) {
                positions.insert(fqn.clone(), unique_fqns.len());
                unique_fqns.push(fqn.clone());
            }
        }
        let fqn_refs: Vec<&str> = unique_fqns.iter().map(String::as_str).collect();
        let usage_scores = self.workspace_index.get_usage_scores(&fqn_refs);

        let typed = trailing_identifier(&context.prefix_text);
        for (item, candidates) in items.iter_mut().zip(&usage_candidates) {
            let usage_score = candidates
                .iter()
                .filter_map(|fqn| positions.get(fqn).map(|index| usage_scores[*index]))
                .fold(0.0, f64::max);
            let score = self.calculate_score(item, context, usage_score);
            item.sort_text = Some(self.sort_key(item, context, typed, score));
        }

        debug!("Ranking complete");
    }

    /// The lexicographic ordering key (IntelliJ-like):
    ///
    /// 1. deprecated items last;
    /// 2. items whose type fits the expected type first;
    /// 3. how well the typed text matches (exact, prefix, hump);
    /// 4. locality group: locals, fields, the receiver's own members, then
    ///    inherited ones, `Object`'s last; workspace types, `java.lang`,
    ///    common JDK packages, everything else;
    /// 5. the weighted relevance score (usage, patterns), then the label.
    fn sort_key(
        &self,
        item: &CompletionItem,
        context: &RankingContext,
        typed: &str,
        score: f64,
    ) -> String {
        let metadata = CompletionItemMetadata::from_item(item);
        let deprecated = item.deprecated == Some(true)
            || item
                .tags
                .as_ref()
                .is_some_and(|tags| tags.contains(&CompletionItemTag::DEPRECATED));

        let expected = context.expected_type.is_some() && {
            let type_score = match metadata.as_ref().and_then(|m| m.item_type.as_deref()) {
                Some(item_type) => self.type_compatibility_score_with_type(item_type, context),
                None => self.type_compatibility_score(item, context),
            };
            type_score >= 0.85
        };

        // `filterText` may have been rewritten for hump matches; the label is
        // what the user reads and what the match quality is judged on.
        let name = item
            .label
            .split(['(', ' ', '<'])
            .next()
            .unwrap_or(&item.label);
        // Typing capitals (`HM`, `gEm`) announces hump intent, so a hump match
        // is as good as a prefix match there; for a lowercase run it is a
        // last resort.
        let hump_intended = typed.chars().skip(1).any(char::is_uppercase)
            || (typed.chars().count() >= 2 && typed.chars().all(char::is_uppercase));
        let bucket = match match_name(typed, name) {
            Some(NameMatch::Exact) => 0,
            Some(NameMatch::Prefix) => 1,
            Some(NameMatch::CamelHump) if hump_intended => 1,
            Some(NameMatch::PrefixIgnoreCase) => 2,
            Some(NameMatch::CamelHump) => 3,
            None => 4,
        };

        let group = metadata
            .as_ref()
            .and_then(|m| self.member_group(m, context))
            .or_else(|| metadata.as_ref().and_then(|m| m.group))
            .unwrap_or_else(|| default_group(item));

        let inverted = score.mul_add(-100.0, 10000.0).clamp(0.0, 99999.0) as u32;
        // Among types no other signal distinguishes, the shorter name is the
        // more basic one (`ArrayList` before `ArrayStoreException`).
        let is_type = matches!(
            item.kind,
            Some(
                CompletionItemKind::CLASS
                    | CompletionItemKind::INTERFACE
                    | CompletionItemKind::ENUM
                    | CompletionItemKind::CONSTRUCTOR
            )
        );
        let length = if is_type {
            metadata
                .as_ref()
                .and_then(|m| m.type_hint)
                .map_or_else(|| 100 + name.chars().count().min(99), usize::from)
        } else {
            0
        };
        format!(
            "{}{}{}{:02}{:03}{:05}_{}",
            u8::from(deprecated),
            u8::from(!expected),
            bucket,
            group,
            length,
            inverted,
            item.label
        )
    }

    /// Group of a member relative to the receiver it is completed on.
    fn member_group(
        &self,
        metadata: &CompletionItemMetadata,
        context: &RankingContext,
    ) -> Option<u8> {
        let owner = metadata.declared_in.as_deref()?;
        if owner == "java.lang.Object" {
            return Some(rank_group::OBJECT_MEMBER);
        }
        Some(match context.receiver_type.as_deref() {
            Some(receiver) if receiver == owner => rank_group::OWN_MEMBER,
            _ => rank_group::INHERITED_MEMBER,
        })
    }

    /// Calculate overall relevance score (0.0 to 100.0)
    ///
    /// Uses cached metadata from CompletionItem.data to avoid recalculating static scores.
    fn calculate_score(
        &self,
        item: &CompletionItem,
        context: &RankingContext,
        usage_score: f64,
    ) -> f64 {
        // Try to load cached metadata
        let cached = CompletionItemMetadata::from_item(item);

        // Usage score: use cache or fall back to expensive lookup
        let usage_score = cached
            .as_ref()
            .and_then(|metadata| metadata.usage_score)
            .unwrap_or(usage_score)
            * 40.0;

        // Alphabetical score: use cache or recalculate (trivial)
        let alpha_score = cached
            .as_ref()
            .map_or_else(|| self.alphabetical_score(item), |m| m.alphabetical_score)
            * 10.0;

        // Type compatibility: can use cached item_type
        let type_score = if let Some(ref meta) = cached {
            if let Some(ref item_type) = meta.item_type {
                self.type_compatibility_score_with_type(item_type, context)
            } else {
                self.type_compatibility_score(item, context)
            }
        } else {
            self.type_compatibility_score(item, context)
        } * 30.0;

        // Pattern score: always context-dependent, no caching
        let pattern_score = self.pattern_score(item, context) * 20.0;

        let total = usage_score + type_score + pattern_score + alpha_score;

        debug!(
            "Score for '{}': {:.1} (usage={:.1}, type={:.1}, pattern={:.1}, alpha={:.1})",
            item.label, total, usage_score, type_score, pattern_score, alpha_score
        );

        total
    }

    /// Calculate type compatibility score (0.0 to 1.0)
    ///
    /// Items matching the expected type score higher.
    fn type_compatibility_score(&self, item: &CompletionItem, context: &RankingContext) -> f64 {
        let expected_type = match &context.expected_type {
            Some(t) => t,
            None => return 0.5, // No expected type = neutral score
        };

        // Extract type from item
        let item_type = self.extract_item_type(item);

        if item_type.is_none() {
            return 0.3; // Unknown type = slightly below neutral
        }

        let item_type = item_type.unwrap();

        // Exact match (highest priority)
        if item_type == *expected_type {
            return 1.0;
        }

        // Check type hierarchy: is item_type a subtype of expected_type?
        if self.is_type_compatible(&item_type, expected_type) {
            return 0.9; // Compatible subtype
        }

        // Simple name extraction for partial matching
        let item_simple = item_type.rsplit('.').next().unwrap_or(&item_type);
        let expected_simple = expected_type.rsplit('.').next().unwrap_or(expected_type);

        // Check if simple names match exactly (FQN vs simple name case)
        if item_simple == expected_simple {
            return 0.85;
        }

        // Partial match: only when simple name is a suffix (e.g., StringBuilder contains Builder)
        // Use stricter check: expected must be a suffix of item or vice versa
        if item_simple.ends_with(expected_simple) || expected_simple.ends_with(item_simple) {
            return 0.6; // Lower score for partial match
        }

        // No match
        0.0
    }

    /// Calculate type compatibility score using cached type string (0.0 to 1.0)
    ///
    /// Same logic as type_compatibility_score but uses pre-extracted type from cache.
    fn type_compatibility_score_with_type(&self, item_type: &str, context: &RankingContext) -> f64 {
        let expected_type = match &context.expected_type {
            Some(t) => t,
            None => return 0.5, // No expected type = neutral score
        };

        // Exact match (highest priority)
        if item_type == expected_type {
            return 1.0;
        }

        // Check type hierarchy: is item_type a subtype of expected_type?
        if self.is_type_compatible(item_type, expected_type) {
            return 0.9; // Compatible subtype
        }

        // Simple name extraction for partial matching
        let item_simple = item_type.rsplit('.').next().unwrap_or(item_type);
        let expected_simple = expected_type.rsplit('.').next().unwrap_or(expected_type);

        // Check if simple names match exactly (FQN vs simple name case)
        if item_simple == expected_simple {
            return 0.85;
        }

        // Partial match: only when simple name is a suffix (e.g., StringBuilder contains Builder)
        // Use stricter check: expected must be a suffix of item or vice versa
        if item_simple.ends_with(expected_simple) || expected_simple.ends_with(item_simple) {
            return 0.6; // Lower score for partial match
        }

        // No match
        0.0
    }

    /// Check if a type is compatible with the expected type using type hierarchy
    ///
    /// Returns true if item_type is a subtype of expected_type (implements interface or extends class)
    fn is_type_compatible(&self, item_type: &str, expected_type: &str) -> bool {
        // Check common Java type hierarchies first (hardcoded for performance)
        // This works even when workspace index is empty
        if self.is_known_subtype(item_type, expected_type) {
            return true;
        }

        // Get FQNs for item_type (there might be multiple with same simple name)
        let item_fqns = self.workspace_index.get_fqns_by_simple_name(item_type);

        if item_fqns.is_empty() {
            return false;
        }

        // Get FQNs for expected_type
        let expected_fqns = self.workspace_index.get_fqns_by_simple_name(expected_type);

        if expected_fqns.is_empty() {
            return false;
        }

        // Check if any item_type implements/extends any expected_type
        for item_fqn in &item_fqns {
            if let Some(symbol) = self.workspace_index.get_symbol(item_fqn) {
                // Check if item implements expected interface
                for iface in &symbol.interfaces {
                    if expected_fqns.iter().any(|e| e == iface.as_ref()) {
                        return true;
                    }
                    // Also check simple name match for common interfaces
                    if iface.ends_with(&format!(".{expected_type}")) {
                        return true;
                    }
                }

                // Check if item extends expected class
                if let Some(parent) = &symbol.parent_class {
                    if expected_fqns.iter().any(|e| e == parent.as_ref()) {
                        return true;
                    }
                    // Also check simple name match
                    if parent.ends_with(&format!(".{expected_type}")) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Check for known Java type hierarchies
    ///
    /// This handles common cases without needing workspace index lookup.
    fn is_known_subtype(&self, item_type: &str, expected_type: &str) -> bool {
        // Object is the root of all reference types
        if expected_type == "Object" && !self.is_primitive_type(item_type) {
            return true;
        }

        // Common collection hierarchies
        match expected_type {
            "List" => matches!(
                item_type,
                "ArrayList" | "LinkedList" | "Vector" | "Stack" | "CopyOnWriteArrayList"
            ),
            "Set" => matches!(
                item_type,
                "HashSet" | "TreeSet" | "LinkedHashSet" | "EnumSet" | "CopyOnWriteArraySet"
            ),
            "Map" => matches!(
                item_type,
                "HashMap"
                    | "TreeMap"
                    | "LinkedHashMap"
                    | "Hashtable"
                    | "ConcurrentHashMap"
                    | "EnumMap"
            ),
            "Collection" => matches!(
                item_type,
                "List"
                    | "Set"
                    | "Queue"
                    | "Deque"
                    | "ArrayList"
                    | "LinkedList"
                    | "HashSet"
                    | "TreeSet"
            ),
            "Iterable" => matches!(
                item_type,
                "Collection" | "List" | "Set" | "Queue" | "ArrayList" | "LinkedList" | "HashSet"
            ),
            "CharSequence" => matches!(item_type, "String" | "StringBuilder" | "StringBuffer"),
            "Number" => matches!(
                item_type,
                "Integer"
                    | "Long"
                    | "Double"
                    | "Float"
                    | "Short"
                    | "Byte"
                    | "BigInteger"
                    | "BigDecimal"
            ),
            "Comparable" => matches!(
                item_type,
                "String" | "Integer" | "Long" | "Double" | "Float" | "Character" | "Boolean"
            ),
            "Serializable" => true, // Most classes are serializable
            "Runnable" => matches!(item_type, "Thread"),
            "Callable" => false, // Requires lambda or implementation
            _ => false,
        }
    }

    /// Check if a type is a primitive type
    fn is_primitive_type(&self, type_name: &str) -> bool {
        matches!(
            type_name,
            "int" | "long" | "short" | "byte" | "char" | "boolean" | "float" | "double" | "void"
        )
    }

    /// Calculate pattern-based score (0.0 to 1.0)
    ///
    /// Recognizes common Java patterns and ranks accordingly.
    fn pattern_score(&self, item: &CompletionItem, context: &RankingContext) -> f64 {
        let label = &item.label;
        let prefix = &context.prefix_text;

        // Pattern 1: Getter/Setter
        if prefix.ends_with("get") && label.starts_with("get") {
            return 1.0; // Strong match: typing "get" and completing "getName"
        }
        if prefix.ends_with("set") && label.starts_with("set") {
            return 1.0;
        }

        // Pattern 2: Builder pattern
        // Detect builder context by checking:
        // 1. Receiver expression ends with .builder()
        // 2. Receiver type contains "Builder"
        let is_builder_context = context
            .receiver_expression
            .as_ref()
            .is_some_and(|r| r.ends_with("builder()"))
            || context
                .receiver_type
                .as_ref()
                .is_some_and(|t| t.contains("Builder"));

        if is_builder_context {
            if label == "build" {
                return 0.05; // build() ends the chain, so it comes after the setters
            }
            if label.starts_with("with") || label.starts_with("set") || label.starts_with("add") {
                return 0.95; // High priority for builder setter methods
            }
            // Lombok-style fluent setters are plain `name(..)` methods that
            // return the builder itself.
            let receiver_simple = context
                .receiver_type
                .as_deref()
                .and_then(|receiver| receiver.rsplit('.').next());
            let returns = item
                .label_details
                .as_ref()
                .and_then(|details| details.description.as_deref());
            if receiver_simple.is_some() && receiver_simple == returns {
                return 0.9;
            }
        }

        // Pattern 3: Stream API
        if context
            .receiver_expression
            .as_ref()
            .is_some_and(|r| r.ends_with("stream()"))
            && matches!(
                label.as_str(),
                "map" | "filter" | "collect" | "forEach" | "reduce"
            )
        {
            return 0.95; // Common stream operations
        }

        // Pattern 4: Avoid Object methods unless explicitly needed
        if matches!(
            label.as_str(),
            "getClass" | "hashCode" | "notify" | "notifyAll" | "wait"
        ) {
            return 0.1; // Penalize generic Object methods
        }

        // Default: neutral score
        0.5
    }

    /// Calculate alphabetical score (0.0 to 1.0)
    ///
    /// Earlier in alphabet scores slightly higher (as tiebreaker).
    fn alphabetical_score(&self, item: &CompletionItem) -> f64 {
        let label = &item.label;

        // First character position in alphabet (a=1.0, z=0.0)
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

    /// Extract type information from completion item
    fn extract_item_type(&self, item: &CompletionItem) -> Option<String> {
        // Try to extract from detail field
        // Format is typically: "ReturnType methodName" or "Type fieldName"
        if let Some(ref detail) = item.detail {
            let parts: Vec<&str> = detail.split_whitespace().collect();
            if !parts.is_empty() {
                return Some(parts[0].to_string());
            }
        }

        // Fall back to item kind
        match item.kind {
            Some(tower_lsp::lsp_types::CompletionItemKind::CLASS) => Some(item.label.clone()),
            Some(tower_lsp::lsp_types::CompletionItemKind::INTERFACE) => Some(item.label.clone()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_workspace::WorkspaceIndex;

    use tempfile::TempDir;
    use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};

    fn create_test_ranker() -> (CompletionRanker, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let index = WorkspaceIndex::new(temp_dir.path().to_path_buf());
        let ranker = CompletionRanker::new(Arc::new(index));
        (ranker, temp_dir)
    }

    fn create_item(label: &str, kind: CompletionItemKind, detail: Option<&str>) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            kind: Some(kind),
            detail: detail.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    // Ranking scores are fixed constants; these tests intentionally guard exact values.
    #[allow(clippy::float_cmp)]
    fn test_pattern_score_getter() {
        let (ranker, _temp) = create_test_ranker();

        let item = create_item("getName", CompletionItemKind::METHOD, None);
        let context = RankingContext::new("user.get".to_string());

        let score = ranker.pattern_score(&item, &context);
        assert_eq!(score, 1.0); // Perfect match: typing "get", completing "getName"
    }

    #[test]
    // Ranking scores are fixed constants; these tests intentionally guard exact values.
    #[allow(clippy::float_cmp)]
    fn test_pattern_score_builder() {
        let (ranker, _temp) = create_test_ranker();

        let item = create_item("withName", CompletionItemKind::METHOD, None);
        let context = RankingContext::new("User.builder().".to_string())
            .with_receiver("User.builder()".to_string());

        let score = ranker.pattern_score(&item, &context);
        assert_eq!(score, 0.95); // Builder method (boosted from 0.9)
    }

    #[test]
    // Ranking scores are fixed constants; these tests intentionally guard exact values.
    #[allow(clippy::float_cmp)]
    fn test_pattern_score_object_methods_penalized() {
        let (ranker, _temp) = create_test_ranker();

        let item = create_item("getClass", CompletionItemKind::METHOD, None);
        let context = RankingContext::new("user.".to_string());

        let score = ranker.pattern_score(&item, &context);
        assert_eq!(score, 0.1); // Penalize Object methods
    }

    #[test]
    fn test_alphabetical_score() {
        let (ranker, _temp) = create_test_ranker();

        let item_a = create_item("aMethod", CompletionItemKind::METHOD, None);
        let item_z = create_item("zMethod", CompletionItemKind::METHOD, None);

        let score_a = ranker.alphabetical_score(&item_a);
        let score_z = ranker.alphabetical_score(&item_z);

        assert!(score_a > score_z); // 'a' scores higher than 'z'
    }

    #[test]
    // Ranking scores are fixed constants; these tests intentionally guard exact values.
    #[allow(clippy::float_cmp)]
    fn test_type_compatibility_exact_match() {
        let (ranker, _temp) = create_test_ranker();

        let item = create_item("ArrayList", CompletionItemKind::CLASS, Some("ArrayList"));
        let context = RankingContext::new("List list = new ".to_string())
            .with_expected_type("ArrayList".to_string());

        let score = ranker.type_compatibility_score(&item, &context);
        assert_eq!(score, 1.0); // Exact type match
    }

    #[test]
    fn test_ranking_updates_sort_text() {
        let (ranker, _temp) = create_test_ranker();

        let mut items = vec![
            create_item("zMethod", CompletionItemKind::METHOD, None),
            create_item("aMethod", CompletionItemKind::METHOD, None),
        ];

        let context = RankingContext::new("obj.".to_string());
        ranker.rank(&mut items, &context);

        // Both should have sort_text set
        assert!(items[0].sort_text.is_some());
        assert!(items[1].sort_text.is_some());

        // Items are not reordered, just sort_text is set
        // LSP client will sort by sort_text
    }

    #[test]
    fn test_known_subtype_list() {
        let (ranker, _temp) = create_test_ranker();

        // ArrayList is a List
        assert!(ranker.is_known_subtype("ArrayList", "List"));
        assert!(ranker.is_known_subtype("LinkedList", "List"));
        // But List is not a String
        assert!(!ranker.is_known_subtype("List", "String"));
    }

    #[test]
    fn test_known_subtype_map() {
        let (ranker, _temp) = create_test_ranker();

        assert!(ranker.is_known_subtype("HashMap", "Map"));
        assert!(ranker.is_known_subtype("TreeMap", "Map"));
        assert!(ranker.is_known_subtype("ConcurrentHashMap", "Map"));
    }

    #[test]
    fn test_known_subtype_object() {
        let (ranker, _temp) = create_test_ranker();

        // Everything extends Object
        assert!(ranker.is_known_subtype("String", "Object"));
        assert!(ranker.is_known_subtype("ArrayList", "Object"));
        // But primitives don't
        assert!(!ranker.is_known_subtype("int", "Object"));
    }

    #[test]
    fn test_type_compatibility_subtype() {
        let (ranker, _temp) = create_test_ranker();

        // ArrayList item when expecting List
        let item = create_item("ArrayList", CompletionItemKind::CLASS, Some("ArrayList"));
        let context = RankingContext::new("List list = new ".to_string())
            .with_expected_type("List".to_string());

        let score = ranker.type_compatibility_score(&item, &context);
        assert!(
            score >= 0.8,
            "ArrayList should be compatible with List (score: {})",
            score
        );
    }

    #[test]
    fn test_is_primitive_type() {
        let (ranker, _temp) = create_test_ranker();

        assert!(ranker.is_primitive_type("int"));
        assert!(ranker.is_primitive_type("boolean"));
        assert!(ranker.is_primitive_type("double"));
        assert!(!ranker.is_primitive_type("String"));
        assert!(!ranker.is_primitive_type("Integer"));
    }
}
