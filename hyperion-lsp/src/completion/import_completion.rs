// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Completion inside `import` declarations: packages, classes, and (for
//! `import static`) the static members of a class.

use crate::completion::type_search::{completion_kind, is_internal_package};
use hyperion_workspace::name_match::match_name;
use hyperion_workspace::WorkspaceIndex;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionItemTag,
    InsertTextFormat, SymbolKind,
};

/// Where in an import declaration the cursor is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPosition {
    /// Everything before the last `.` (`java.util`), empty at the start.
    pub qualifier: String,
    /// The identifier being typed after the last `.`.
    pub prefix: String,
    /// `import static ...`
    pub is_static: bool,
}

/// Recognize `import [static] a.b.pre|` on the current line.
#[must_use]
pub fn detect(text_before_cursor: &str) -> Option<ImportPosition> {
    let line_start = text_before_cursor.rfind('\n').map_or(0, |i| i + 1);
    let line = text_before_cursor[line_start..].trim_start();
    let rest = line.strip_prefix("import")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut rest = rest.trim_start();
    let mut is_static = false;
    if let Some(after) = rest.strip_prefix("static") {
        if after.starts_with(char::is_whitespace) {
            is_static = true;
            rest = after.trim_start();
        }
    }
    if !rest
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '$')
    {
        return None;
    }
    let (qualifier, prefix) = rest.rsplit_once('.').unwrap_or(("", rest));
    Some(ImportPosition {
        qualifier: qualifier.to_string(),
        prefix: prefix.to_string(),
        is_static,
    })
}

fn item(
    label: &str,
    kind: CompletionItemKind,
    detail: Option<String>,
    order: u8,
) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        label_details: detail.map(|description| CompletionItemLabelDetails {
            detail: None,
            description: Some(description),
        }),
        insert_text: Some(label.to_string()),
        insert_text_format: Some(InsertTextFormat::PLAIN_TEXT),
        sort_text: Some(format!("{order}_{label}")),
        ..Default::default()
    }
}

/// Completion items for an import position.
pub fn complete(index: &WorkspaceIndex, position: &ImportPosition) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let qualifier = position.qualifier.as_str();
    let matches = |name: &str| match_name(&position.prefix, name).is_some();

    if index.has_class(qualifier) {
        // `import [static] pkg.Type.|`: nested types and, for static imports,
        // static members.
        for member in index.get_class_members(qualifier) {
            let name = member.simple_name.as_ref();
            if !member.modifiers.is_public && !member.modifiers.is_static
                || !matches(name)
                || name.contains('$')
                || member.kind == SymbolKind::CONSTRUCTOR
            {
                continue;
            }
            let is_type = hyperion_workspace::is_type_declaration_kind(member.kind);
            if !(is_type || position.is_static && member.modifiers.is_static) {
                continue;
            }
            let kind = if is_type {
                completion_kind(&member)
            } else if member.kind == SymbolKind::METHOD {
                CompletionItemKind::METHOD
            } else if member.kind == SymbolKind::ENUM_MEMBER {
                CompletionItemKind::ENUM_MEMBER
            } else {
                CompletionItemKind::FIELD
            };
            let mut entry = item(name, kind, Some(qualifier.to_string()), u8::from(!is_type));
            if member.is_deprecated {
                entry.tags = Some(vec![CompletionItemTag::DEPRECATED]);
                entry.sort_text = Some(format!("9_{name}"));
            }
            items.push(entry);
        }
        items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
        items.dedup_by(|a, b| a.label == b.label && a.kind == b.kind);
        if matches("*") || position.prefix.is_empty() {
            items.push(item("*", CompletionItemKind::MODULE, None, 9));
        }
        return items;
    }

    for child in index.child_package_names(qualifier) {
        let full = if qualifier.is_empty() {
            child.clone()
        } else {
            format!("{qualifier}.{child}")
        };
        // Class names appear as package keys for their nested types.
        if child.chars().next().is_some_and(char::is_uppercase)
            || is_internal_package(&full)
            || !matches(&child)
        {
            continue;
        }
        items.push(item(&child, CompletionItemKind::MODULE, Some(full), 1));
    }
    if !qualifier.is_empty() {
        for class_name in index.get_package_class_names(qualifier) {
            let Some(symbol) = index.get_symbol(&format!("{qualifier}.{class_name}")) else {
                continue;
            };
            let name = symbol.simple_name.as_ref();
            if !symbol.modifiers.is_public || name.contains('$') || !matches(name) {
                continue;
            }
            let mut entry = item(
                name,
                completion_kind(&symbol),
                Some(qualifier.to_string()),
                0,
            );
            if symbol.is_deprecated {
                entry.tags = Some(vec![CompletionItemTag::DEPRECATED]);
                entry.sort_text = Some(format!("9_{name}"));
            }
            items.push(entry);
        }
        if !position.is_static && (position.prefix.is_empty() || position.prefix == "*") {
            items.push(item("*", CompletionItemKind::MODULE, None, 9));
        }
    }
    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    items
}

#[cfg(test)]
mod tests {
    use super::{detect, ImportPosition};

    fn position(qualifier: &str, prefix: &str, is_static: bool) -> Option<ImportPosition> {
        Some(ImportPosition {
            qualifier: qualifier.to_string(),
            prefix: prefix.to_string(),
            is_static,
        })
    }

    #[test]
    fn detects_import_positions() {
        assert_eq!(
            detect("import java.util."),
            position("java.util", "", false)
        );
        assert_eq!(detect("import java.ut"), position("java", "ut", false));
        assert_eq!(detect("import "), position("", "", false));
        assert_eq!(
            detect("package p;\nimport static java.lang.Math.ab"),
            position("java.lang.Math", "ab", true)
        );
    }

    #[test]
    fn ignores_everything_else() {
        assert_eq!(detect("importer."), None);
        assert_eq!(detect("import java.util.List;"), None);
        assert_eq!(detect("x = import.foo"), None);
        assert_eq!(detect("class A { void m() { foo."), None);
    }
}
