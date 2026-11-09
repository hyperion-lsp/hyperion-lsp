// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Move Refactoring Actions
//!
//! This module provides the move-method refactoring action. Moving a class
//! to another package lives in `crate::file_move` (shared with file-explorer moves).

use std::collections::HashMap;
use std::sync::Arc;

use tower_lsp::lsp_types::{CodeAction, Range, TextEdit as LspTextEdit, Url, WorkspaceEdit};
use tracing::debug;

use hyperion_workspace::{DeclarationNamespace, SymbolTarget, WorkspaceIndex};

use super::kinds;
use super::utils::position_to_offset;
use crate::position_utils::offset_to_position;

// ============================================================================
// Move Method Refactoring
// ============================================================================

/// Information about a method found at a position
struct MethodInfo {
    name: String,
    params: Vec<(String, String)>, // (type, name)
    return_type: Option<String>,
    body: String,
    start_offset: usize,
    end_offset: usize,
}

/// Generate "Move Method to Class" refactoring actions
///
/// Suggests moving a method to classes that are parameters of the method
pub fn move_method_actions(
    range: &Range,
    uri: &Url,
    source_text: &str,
    workspace_index: &Arc<WorkspaceIndex>,
) -> Vec<CodeAction> {
    let mut actions = Vec::new();

    let start_offset = match position_to_offset(source_text, range.start).ok() {
        Some(o) => o as usize,
        None => return actions,
    };

    // Find method at cursor position
    let method_info = match find_method_at_position(source_text, start_offset) {
        Some(info) => info,
        None => return actions,
    };

    debug!("Found method '{}' for move refactoring", method_info.name);

    // Find candidate target classes from method parameters
    let candidates = find_candidate_target_classes(&method_info);

    for (param_type, param_name) in candidates {
        if let Some(action) = create_move_method_action(
            &method_info,
            &param_type,
            &param_name,
            uri,
            source_text,
            workspace_index,
        ) {
            actions.push(action);
        }
    }

    actions
}

/// Find a method declaration at the given position
fn find_method_at_position(source: &str, position: usize) -> Option<MethodInfo> {
    // Find the line containing the position
    let line_start = source[..position].rfind('\n').map_or(0, |p| p + 1);
    let line_end = source[position..]
        .find('\n')
        .map_or(source.len(), |p| position + p);
    let current_line = &source[line_start..line_end];

    // Check if we're on a method declaration line (has return type and parentheses)
    if !current_line.contains('(') || current_line.contains("class ") {
        return None;
    }

    // Parse method signature
    let trimmed = current_line.trim();
    if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with('@') {
        return None;
    }

    // Find method name (word before '(')
    let paren_pos = trimmed.find('(')?;
    let before_paren = trimmed[..paren_pos].trim();
    let parts: Vec<&str> = before_paren.split_whitespace().collect();

    if parts.len() < 2 {
        return None;
    }

    let method_name = (*parts.last()?).to_string();
    let return_type = if parts.len() >= 2 && parts[parts.len() - 2] != "void" {
        Some(parts[parts.len() - 2].to_string())
    } else if parts.contains(&"void") {
        None
    } else {
        Some(parts[parts.len() - 2].to_string())
    };

    // Extract parameters
    let params_start = paren_pos + 1;
    let params_end = trimmed[params_start..].find(')')? + params_start;
    let params_str = &trimmed[params_start..params_end];

    let params = parse_params(params_str);

    // Find method body bounds
    let method_start = line_start;
    let body_start = source[line_start..].find('{')? + line_start;
    let body_end = find_matching_brace(source, body_start)?;

    let body = source[body_start..=body_end].to_string();

    Some(MethodInfo {
        name: method_name,
        params,
        return_type,
        body,
        start_offset: method_start,
        end_offset: body_end + 1,
    })
}

/// Parse method parameters from string like "User user, String name"
fn parse_params(params_str: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();

    for param in params_str.split(',') {
        let param = param.trim();
        if param.is_empty() {
            continue;
        }

        let parts: Vec<&str> = param.split_whitespace().collect();
        if parts.len() >= 2 {
            let param_type = parts[..parts.len() - 1].join(" ");
            let param_name = (*parts.last().unwrap()).to_string();
            params.push((param_type, param_name));
        }
    }

    params
}

/// Find matching closing brace
fn find_matching_brace(source: &str, start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut prev_char = ' ';

    for (i, c) in source[start..].char_indices() {
        match c {
            '"' if prev_char != '\\' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i);
                }
            }
            _ => {}
        }
        prev_char = c;
    }

    None
}

/// Find candidate target classes from method parameters
fn find_candidate_target_classes(method_info: &MethodInfo) -> Vec<(String, String)> {
    let mut candidates = Vec::new();

    for (param_type, param_name) in &method_info.params {
        // Skip primitive types and common utility types
        let type_name = param_type.split('<').next().unwrap_or(param_type);
        if is_primitive_or_utility(type_name) {
            continue;
        }

        candidates.push((type_name.to_string(), param_name.clone()));
    }

    candidates
}

/// Check if type is primitive or common utility type
fn is_primitive_or_utility(type_name: &str) -> bool {
    matches!(
        type_name,
        "int"
            | "long"
            | "short"
            | "byte"
            | "float"
            | "double"
            | "boolean"
            | "char"
            | "Integer"
            | "Long"
            | "Short"
            | "Byte"
            | "Float"
            | "Double"
            | "Boolean"
            | "Character"
            | "String"
            | "Object"
            | "List"
            | "Set"
            | "Map"
            | "Collection"
            | "Optional"
    )
}

/// Create a move method action for a specific target class
fn create_move_method_action(
    method_info: &MethodInfo,
    target_type: &str,
    param_name: &str,
    uri: &Url,
    source_text: &str,
    workspace_index: &Arc<WorkspaceIndex>,
) -> Option<CodeAction> {
    let mut changes: HashMap<Url, Vec<LspTextEdit>> = HashMap::new();

    // Generate the new method for the target class
    let new_method = generate_moved_method(method_info, target_type, param_name);

    // Remove the original method from this file
    changes.insert(
        uri.clone(),
        vec![LspTextEdit {
            range: Range {
                start: offset_to_position(source_text, method_info.start_offset),
                end: offset_to_position(source_text, method_info.end_offset),
            },
            new_text: String::new(),
        }],
    );

    // Find the target class file and add the method there
    let usages = workspace_index
        .find_current_usages_for_symbol_target(&SymbolTarget::new(
            target_type,
            DeclarationNamespace::Type,
        ))
        .ok()?;
    debug!(
        "Found {} usages of {} for move method target",
        usages.locations().count(),
        target_type
    );

    // Find the class definition file
    for file in &usages.files {
        let indexed = workspace_index.get_indexed_revision_content(file.revision)?;
        if indexed.content.contains(&format!("class {target_type} "))
            || indexed.content.contains(&format!("class {target_type}<"))
        {
            if let Some(insert_pos) = find_class_insert_position(&indexed.content) {
                let usage_uri = Url::from_file_path(&indexed.file_path).ok()?;
                changes.entry(usage_uri).or_default().push(LspTextEdit {
                    range: Range {
                        start: offset_to_position(&indexed.content, insert_pos),
                        end: offset_to_position(&indexed.content, insert_pos),
                    },
                    new_text: format!("\n{new_method}\n"),
                });
                break;
            }
        }
    }
    if !workspace_index.current_usage_locations_are_current(&usages) {
        return None;
    }

    Some(CodeAction {
        title: format!("Move '{}' to class '{}'", method_info.name, target_type),
        kind: Some(kinds::refactor_move()),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Generate the method body for the target class
fn generate_moved_method(method_info: &MethodInfo, _target_type: &str, param_name: &str) -> String {
    // Build new parameter list without the target parameter
    let new_params: Vec<String> = method_info
        .params
        .iter()
        .filter(|(_, name)| name != param_name)
        .map(|(typ, name)| format!("{typ} {name}"))
        .collect();

    let return_type = method_info.return_type.as_deref().unwrap_or("void");

    // Replace references to param_name with "this" in the body
    let new_body = method_info.body.replace(&format!("{param_name}."), "this.");

    format!(
        "    public {} {}({}) {}",
        return_type,
        method_info.name,
        new_params.join(", "),
        new_body
    )
}

/// Find position to insert a new method in a class (before the last closing brace)
fn find_class_insert_position(source: &str) -> Option<usize> {
    // Find the last closing brace that ends the class
    let mut last_brace = None;
    let mut depth = 0;
    let mut in_string = false;
    let mut prev_char = ' ';

    for (i, c) in source.char_indices() {
        match c {
            '"' if prev_char != '\\' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    last_brace = Some(i);
                }
            }
            _ => {}
        }
        prev_char = c;
    }

    last_brace
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Move Method Tests
    // ========================================================================

    #[test]
    fn test_parse_params() {
        let params = parse_params("User user, String name");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], ("User".to_string(), "user".to_string()));
        assert_eq!(params[1], ("String".to_string(), "name".to_string()));
    }

    #[test]
    fn test_parse_params_empty() {
        let params = parse_params("");
        assert!(params.is_empty());
    }

    #[test]
    fn test_find_matching_brace() {
        let source = "{ foo { bar } }";
        assert_eq!(find_matching_brace(source, 0), Some(14));
    }

    #[test]
    fn test_find_matching_brace_with_strings() {
        let source = r#"{ "}" }"#;
        assert_eq!(find_matching_brace(source, 0), Some(6));
    }

    #[test]
    fn test_is_primitive_or_utility() {
        assert!(is_primitive_or_utility("int"));
        assert!(is_primitive_or_utility("String"));
        assert!(is_primitive_or_utility("List"));
        assert!(!is_primitive_or_utility("User"));
        assert!(!is_primitive_or_utility("OrderService"));
    }

    #[test]
    fn test_find_candidate_target_classes() {
        let method_info = MethodInfo {
            name: "process".to_string(),
            params: vec![
                ("User".to_string(), "user".to_string()),
                ("String".to_string(), "name".to_string()),
            ],
            return_type: None,
            body: "{}".to_string(),
            start_offset: 0,
            end_offset: 0,
        };

        let candidates = find_candidate_target_classes(&method_info);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0], ("User".to_string(), "user".to_string()));
    }

    #[test]
    fn test_generate_moved_method() {
        let method_info = MethodInfo {
            name: "getName".to_string(),
            params: vec![("User".to_string(), "user".to_string())],
            return_type: Some("String".to_string()),
            body: "{ return user.name; }".to_string(),
            start_offset: 0,
            end_offset: 0,
        };

        let result = generate_moved_method(&method_info, "User", "user");
        assert!(result.contains("public String getName()"));
        assert!(result.contains("this.name"));
    }

    #[test]
    fn test_find_class_insert_position() {
        let source = "class Test {\n    void foo() {}\n}";
        let pos = find_class_insert_position(source);
        assert_eq!(pos, Some(31)); // Position of the last }
    }
}
