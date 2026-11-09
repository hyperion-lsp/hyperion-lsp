// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Surround With Code Actions
//!
//! This module provides "Surround with" refactoring actions that wrap selected code
//! with control flow statements like if, try-catch, while, etc.

use std::collections::HashMap;
use tower_lsp::lsp_types::{CodeAction, Range, TextEdit as LspTextEdit, Url, WorkspaceEdit};

use super::kinds;
use super::utils::{get_text_in_range, position_to_offset};

/// Generate "Surround With" code actions for a selected range
pub fn surround_with_actions(range: &Range, uri: &Url, source_text: &str) -> Vec<CodeAction> {
    let mut actions = Vec::new();

    let selected_text = match get_text_in_range(source_text, range) {
        Some(text) => text,
        None => return actions,
    };

    let trimmed = selected_text.trim();
    if trimmed.is_empty() {
        return actions;
    }

    // Get indentation of selected code
    let start_offset = match position_to_offset(source_text, range.start).ok() {
        Some(o) => o as usize,
        None => return actions,
    };

    let line_start = source_text[..start_offset].rfind('\n').map_or(0, |p| p + 1);

    let indent: String = source_text[line_start..]
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .collect();

    // Re-indent the selected code for the body
    let body_indent = format!("{indent}    ");
    let indented_body: String = trimmed
        .lines()
        .map(|line| {
            let stripped = line.trim_start();
            if stripped.is_empty() {
                String::new()
            } else {
                format!("{body_indent}{stripped}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Surround with if
    actions.push(create_surround_action(
        "Surround with if",
        range,
        uri,
        &format!("{indent}if (condition) {{\n{indented_body}\n{indent}}}"),
    ));

    // Surround with if-else
    actions.push(create_surround_action(
        "Surround with if-else",
        range,
        uri,
        &format!(
            "{indent}if (condition) {{\n{indented_body}\n{indent}}} else {{\n{body_indent}\n{indent}}}"
        ),
    ));

    // Surround with try-catch
    actions.push(create_surround_action(
        "Surround with try-catch",
        range,
        uri,
        &format!(
            "{indent}try {{\n{indented_body}\n{indent}}} catch (Exception e) {{\n{indent}    e.printStackTrace();\n{indent}}}"
        ),
    ));

    // Surround with try-finally
    actions.push(create_surround_action(
        "Surround with try-finally",
        range,
        uri,
        &format!(
            "{indent}try {{\n{indented_body}\n{indent}}} finally {{\n{body_indent}\n{indent}}}"
        ),
    ));

    // Surround with while
    actions.push(create_surround_action(
        "Surround with while",
        range,
        uri,
        &format!("{indent}while (condition) {{\n{indented_body}\n{indent}}}"),
    ));

    // Surround with for
    actions.push(create_surround_action(
        "Surround with for",
        range,
        uri,
        &format!("{indent}for (int i = 0; i < count; i++) {{\n{indented_body}\n{indent}}}"),
    ));

    // Surround with synchronized
    actions.push(create_surround_action(
        "Surround with synchronized",
        range,
        uri,
        &format!("{indent}synchronized (this) {{\n{indented_body}\n{indent}}}"),
    ));

    actions
}

/// Helper to create a surround action
fn create_surround_action(title: &str, range: &Range, uri: &Url, new_text: &str) -> CodeAction {
    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![LspTextEdit {
            range: *range,
            new_text: new_text.to_string(),
        }],
    );

    CodeAction {
        title: title.to_string(),
        kind: Some(kinds::refactor_surround()),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: None,
        disabled: None,
        data: None,
    }
}
