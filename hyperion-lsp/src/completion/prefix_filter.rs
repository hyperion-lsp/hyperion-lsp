// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Server-side prefix and camel-hump filtering of completion items.
//!
//! The request is re-issued on every keystroke (`isIncomplete`), so the
//! server can afford to send only what matches what has been typed. Clients
//! that only do strict prefix filtering on `filterText` would otherwise
//! hide camel-hump matches like `gEm` -> `getEmail`, so a hump-only match is
//! given a `filterText` that starts with exactly what the user typed.

use hyperion_workspace::name_match::{match_name, NameMatch};
use tower_lsp::lsp_types::CompletionItem;

/// The identifier a typed pattern is matched against: `filterText` if the
/// generator set one, else the label without any signature suffix.
pub fn match_key(item: &CompletionItem) -> &str {
    let text = item.filter_text.as_deref().unwrap_or(&item.label);
    text.split(['(', ' ', '<']).next().unwrap_or(text)
}

/// Keep only items matching `prefix`, and make hump-only matches survive a
/// client's own prefix filter.
pub fn retain_matching(items: &mut Vec<CompletionItem>, prefix: &str) {
    if prefix.is_empty() {
        return;
    }
    items.retain_mut(|item| match match_name(prefix, match_key(item)) {
        None => false,
        Some(NameMatch::CamelHump) => {
            let key = match_key(item).to_string();
            item.filter_text = Some(format!("{prefix}{key}"));
            true
        }
        Some(_) => true,
    });
}
