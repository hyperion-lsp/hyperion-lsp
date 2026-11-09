// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Chain Completion Snippets
//!
//! Provides snippet completions for common method chains on Stream, Optional, and other types.

use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

/// Generate completion snippets for Stream operations
///
/// These are common patterns used after `.stream()` calls
pub fn stream_chain_snippets() -> Vec<CompletionItem> {
    vec![
        create_chain_snippet(
            "collect(Collectors.toList())",
            "collect(Collectors.toList())",
            "Collect stream to List",
            "java.util.stream",
        ),
        create_chain_snippet(
            "collect(Collectors.toSet())",
            "collect(Collectors.toSet())",
            "Collect stream to Set",
            "java.util.stream",
        ),
        create_chain_snippet(
            "collect(Collectors.toMap(...))",
            "collect(Collectors.toMap(${1:keyMapper}, ${2:valueMapper}))",
            "Collect stream to Map",
            "java.util.stream",
        ),
        create_chain_snippet(
            "findFirst().orElse(null)",
            "findFirst().orElse(null)",
            "Find first element or null",
            "java.util.stream",
        ),
        create_chain_snippet(
            "findFirst().orElseThrow()",
            "findFirst().orElseThrow()",
            "Find first element or throw NoSuchElementException",
            "java.util.stream",
        ),
        create_chain_snippet(
            "filter(...)",
            "filter(${1:element} -> ${0})",
            "Filter stream elements",
            "java.util.stream",
        ),
        create_chain_snippet(
            "map(...)",
            "map(${1:element} -> ${0})",
            "Map stream elements",
            "java.util.stream",
        ),
        create_chain_snippet(
            "flatMap(...)",
            "flatMap(${1:element} -> ${0})",
            "FlatMap stream elements",
            "java.util.stream",
        ),
        create_chain_snippet(
            "forEach(...)",
            "forEach(${1:element} -> ${0})",
            "Consume each element",
            "java.util.stream",
        ),
        create_chain_snippet(
            "reduce(...)",
            "reduce(${1:identity}, (${2:a}, ${3:b}) -> ${0})",
            "Reduce stream to single value",
            "java.util.stream",
        ),
        create_chain_snippet(
            "sorted()",
            "sorted()",
            "Sort stream elements (natural order)",
            "java.util.stream",
        ),
        create_chain_snippet(
            "sorted(Comparator...)",
            "sorted(Comparator.comparing(${1:keyExtractor}))",
            "Sort stream by key",
            "java.util.stream",
        ),
        create_chain_snippet(
            "distinct()",
            "distinct()",
            "Remove duplicate elements",
            "java.util.stream",
        ),
        create_chain_snippet(
            "limit(...)",
            "limit(${1:maxSize})",
            "Limit stream to max elements",
            "java.util.stream",
        ),
        create_chain_snippet(
            "count()",
            "count()",
            "Count stream elements",
            "java.util.stream",
        ),
        create_chain_snippet(
            "anyMatch(...)",
            "anyMatch(${1:element} -> ${0})",
            "Check if any element matches",
            "java.util.stream",
        ),
        create_chain_snippet(
            "allMatch(...)",
            "allMatch(${1:element} -> ${0})",
            "Check if all elements match",
            "java.util.stream",
        ),
        create_chain_snippet(
            "noneMatch(...)",
            "noneMatch(${1:element} -> ${0})",
            "Check if no elements match",
            "java.util.stream",
        ),
    ]
}

/// Generate completion snippets for Optional operations
///
/// These are common patterns used on Optional values
pub fn optional_chain_snippets() -> Vec<CompletionItem> {
    vec![
        create_chain_snippet(
            "orElse(null)",
            "orElse(null)",
            "Get value or null",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "orElse(...)",
            "orElse(${1:defaultValue})",
            "Get value or default",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "orElseThrow()",
            "orElseThrow()",
            "Get value or throw NoSuchElementException",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "orElseThrow(Exception::new)",
            "orElseThrow(${1:RuntimeException}::new)",
            "Get value or throw custom exception",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "orElseGet(...)",
            "orElseGet(() -> ${0})",
            "Get value or compute default",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "map(...)",
            "map(${1:value} -> ${0})",
            "Transform optional value",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "flatMap(...)",
            "flatMap(${1:value} -> ${0})",
            "Transform to another Optional",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "filter(...)",
            "filter(${1:value} -> ${0})",
            "Filter optional by predicate",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "ifPresent(...)",
            "ifPresent(${1:value} -> ${0})",
            "Execute action if present",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "ifPresentOrElse(...)",
            "ifPresentOrElse(${1:value} -> ${2}, () -> ${0})",
            "Execute action if present or else",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "isPresent()",
            "isPresent()",
            "Check if value is present",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "isEmpty()",
            "isEmpty()",
            "Check if value is empty (Java 11+)",
            "java.util.Optional",
        ),
        create_chain_snippet(
            "get()",
            "get()",
            "Get value (throws if empty)",
            "java.util.Optional",
        ),
    ]
}

/// Generate completion snippets for CompletableFuture operations
pub fn completable_future_snippets() -> Vec<CompletionItem> {
    vec![
        create_chain_snippet(
            "thenApply(...)",
            "thenApply(${1:result} -> ${0})",
            "Transform result",
            "java.util.concurrent.CompletableFuture",
        ),
        create_chain_snippet(
            "thenAccept(...)",
            "thenAccept(${1:result} -> ${0})",
            "Consume result",
            "java.util.concurrent.CompletableFuture",
        ),
        create_chain_snippet(
            "thenCompose(...)",
            "thenCompose(${1:result} -> ${0})",
            "Chain another CompletableFuture",
            "java.util.concurrent.CompletableFuture",
        ),
        create_chain_snippet(
            "thenCombine(...)",
            "thenCombine(${1:other}, (${2:a}, ${3:b}) -> ${0})",
            "Combine with another future",
            "java.util.concurrent.CompletableFuture",
        ),
        create_chain_snippet(
            "exceptionally(...)",
            "exceptionally(${1:ex} -> ${0})",
            "Handle exception",
            "java.util.concurrent.CompletableFuture",
        ),
        create_chain_snippet(
            "handle(...)",
            "handle((${1:result}, ${2:ex}) -> ${0})",
            "Handle result or exception",
            "java.util.concurrent.CompletableFuture",
        ),
        create_chain_snippet(
            "join()",
            "join()",
            "Block and get result",
            "java.util.concurrent.CompletableFuture",
        ),
        create_chain_snippet(
            "get()",
            "get()",
            "Block and get result (throws checked exceptions)",
            "java.util.concurrent.CompletableFuture",
        ),
    ]
}

/// Create a chain snippet completion item
fn create_chain_snippet(
    label: &str,
    insert_text: &str,
    detail: &str,
    source_type: &str,
) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(format!("{detail} - {source_type}")),
        insert_text: Some(insert_text.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        sort_text: Some(format!("1_chain_{label}")), // Slightly lower priority than regular methods
        ..Default::default()
    }
}

/// Check if the receiver type is a Stream or Stream-like type
pub fn is_stream_type(type_name: &str) -> bool {
    let simple_name = type_name
        .rsplit_once('.')
        .map_or(type_name, |(_, simple)| simple);
    matches!(
        simple_name,
        "Stream" | "IntStream" | "LongStream" | "DoubleStream"
    ) || type_name.ends_with(".Stream")
        || type_name.ends_with(".IntStream")
        || type_name.ends_with(".LongStream")
        || type_name.ends_with(".DoubleStream")
}

/// Check if the receiver type is an Optional or Optional-like type
pub fn is_optional_type(type_name: &str) -> bool {
    let simple_name = type_name
        .rsplit_once('.')
        .map_or(type_name, |(_, simple)| simple);
    matches!(
        simple_name,
        "Optional" | "OptionalInt" | "OptionalLong" | "OptionalDouble"
    ) || type_name.ends_with(".Optional")
        || type_name.ends_with(".OptionalInt")
        || type_name.ends_with(".OptionalLong")
        || type_name.ends_with(".OptionalDouble")
}

/// Check if the receiver type is a CompletableFuture
pub fn is_completable_future_type(type_name: &str) -> bool {
    let simple_name = type_name
        .rsplit_once('.')
        .map_or(type_name, |(_, simple)| simple);
    simple_name == "CompletableFuture" || type_name.ends_with(".CompletableFuture")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_snippets_generated() {
        let snippets = stream_chain_snippets();
        assert!(!snippets.is_empty());
        assert!(snippets.iter().any(|s| s.label.contains("collect")));
        assert!(snippets.iter().any(|s| s.label.contains("filter")));
        assert!(snippets.iter().any(|s| s.label.contains("map")));
    }

    #[test]
    fn test_optional_snippets_generated() {
        let snippets = optional_chain_snippets();
        assert!(!snippets.is_empty());
        assert!(snippets.iter().any(|s| s.label.contains("orElse")));
        assert!(snippets.iter().any(|s| s.label.contains("map")));
        assert!(snippets.iter().any(|s| s.label.contains("ifPresent")));
    }

    #[test]
    fn test_is_stream_type() {
        assert!(is_stream_type("Stream"));
        assert!(is_stream_type("java.util.stream.Stream"));
        assert!(is_stream_type("IntStream"));
        assert!(!is_stream_type("String"));
        assert!(!is_stream_type("List"));
    }

    #[test]
    fn test_is_optional_type() {
        assert!(is_optional_type("Optional"));
        assert!(is_optional_type("java.util.Optional"));
        assert!(is_optional_type("OptionalInt"));
        assert!(!is_optional_type("String"));
        assert!(!is_optional_type("List"));
    }

    #[test]
    fn test_is_completable_future_type() {
        assert!(is_completable_future_type("CompletableFuture"));
        assert!(is_completable_future_type(
            "java.util.concurrent.CompletableFuture"
        ));
        assert!(!is_completable_future_type("Future"));
        assert!(!is_completable_future_type("String"));
    }

    #[test]
    fn test_snippet_format() {
        let snippets = stream_chain_snippets();
        for snippet in &snippets {
            assert_eq!(snippet.kind, Some(CompletionItemKind::SNIPPET));
            assert!(snippet.insert_text.is_some());
            assert_eq!(snippet.insert_text_format, Some(InsertTextFormat::SNIPPET));
        }
    }
}
