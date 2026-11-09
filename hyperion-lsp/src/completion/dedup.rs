// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::metadata::CompletionItemMetadata;
use std::collections::HashSet;
use tower_lsp::lsp_types::CompletionItem;

/// Collapse only semantically identical items. Labels are not identities:
/// Java overloads and types from different packages routinely share them.
pub(super) fn deduplicate_completion_items(completions: &mut Vec<CompletionItem>) {
    let mut seen = HashSet::new();
    completions.retain(|item| seen.insert(identity(item)));
}

fn identity(item: &CompletionItem) -> String {
    let metadata = CompletionItemMetadata::from_item(item);
    let semantic_id = metadata
        .as_ref()
        .and_then(|metadata| metadata.semantic_id.as_deref())
        .or_else(|| {
            metadata
                .as_ref()
                .and_then(|metadata| metadata.fqn.as_deref())
        });

    if let Some(id) = semantic_id {
        // An FQN identifies the declaration owner for constructors, not one
        // overload. Behavior-bearing fields remain part of the key as well:
        // snippet/plain-text variants and commands are not interchangeable.
        format!("semantic:{id}:{}", behavior_key(item))
    } else {
        // Metadata-free generator items are identical only when every field
        // that can change insertion behavior agrees.
        format!("fallback:{:?}:{}", item.detail, behavior_key(item))
    }
}

fn behavior_key(item: &CompletionItem) -> String {
    format!(
        "{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}",
        item.label,
        item.kind,
        item.insert_text,
        item.insert_text_format,
        item.text_edit,
        item.additional_text_edits,
        item.command,
        item.commit_characters,
        item.filter_text,
        item.label_details
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{CompletionItemKind, InsertTextFormat, Range, TextEdit};

    fn semantic_item(label: &str, kind: CompletionItemKind, id: &str) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            kind: Some(kind),
            data: CompletionItemMetadata::new(label)
                .with_semantic_id(id.to_string())
                .to_value(),
            ..CompletionItem::default()
        }
    }

    #[test]
    fn preserves_method_overloads() {
        let mut items = vec![
            semantic_item("foo", CompletionItemKind::METHOD, "pkg.C.foo(int)"),
            semantic_item("foo", CompletionItemKind::METHOD, "pkg.C.foo(String)"),
        ];
        deduplicate_completion_items(&mut items);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn preserves_constructor_overloads_and_type_role() {
        let mut no_args = semantic_item("Foo()", CompletionItemKind::CONSTRUCTOR, "pkg.Foo");
        no_args.insert_text = Some("Foo()$0".to_string());
        let mut one_arg = semantic_item("Foo(String)", CompletionItemKind::CONSTRUCTOR, "pkg.Foo");
        one_arg.insert_text = Some("Foo(${1:value})$0".to_string());
        let class = semantic_item("Foo", CompletionItemKind::CLASS, "pkg.Foo");
        let mut items = vec![no_args, one_arg, class];
        deduplicate_completion_items(&mut items);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn preserves_distinct_fallback_behavior() {
        let mut first = CompletionItem {
            label: "generate".to_string(),
            ..CompletionItem::default()
        };
        first.additional_text_edits = Some(vec![TextEdit {
            range: Range::default(),
            new_text: "first".to_string(),
        }]);
        let mut second = first.clone();
        second.additional_text_edits.as_mut().unwrap()[0].new_text = "second".to_string();
        let mut items = vec![first, second];
        deduplicate_completion_items(&mut items);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn preserves_distinct_semantic_insertion_behavior() {
        let mut plain = semantic_item("foo", CompletionItemKind::METHOD, "pkg.C.foo()");
        plain.insert_text = Some("foo()$0".to_string());
        plain.insert_text_format = Some(InsertTextFormat::PLAIN_TEXT);
        let mut snippet = plain.clone();
        snippet.insert_text_format = Some(InsertTextFormat::SNIPPET);
        let mut items = vec![plain, snippet];

        deduplicate_completion_items(&mut items);

        assert_eq!(items.len(), 2);
    }

    #[test]
    fn preserves_same_named_types() {
        let mut items = vec![
            semantic_item("Date", CompletionItemKind::CLASS, "java.util.Date"),
            semantic_item("Date", CompletionItemKind::CLASS, "java.sql.Date"),
        ];
        deduplicate_completion_items(&mut items);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn collapses_exact_duplicates_and_keeps_first() {
        let mut first = semantic_item("Date", CompletionItemKind::CLASS, "java.util.Date");
        first.detail = Some("first".to_string());
        let mut duplicate = semantic_item("Date", CompletionItemKind::CLASS, "java.util.Date");
        duplicate.detail = Some("duplicate".to_string());
        let mut items = vec![first, duplicate];
        deduplicate_completion_items(&mut items);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].detail.as_deref(), Some("first"));
    }
}
