// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Extract Refactoring Actions
//!
//! This module provides extraction refactoring actions:
//! - Extract Variable: Extract an expression into a local variable
//! - Extract Method: Extract selected code into a new method
//! - Introduce Constant: Extract a value into a class constant
//! - Introduce Field: Extract an expression into a class field
//! - Introduce Parameter: Extract an expression into a method parameter

use hyperion_workspace::{DeclarationNamespace, SymbolTarget, WorkspaceIndex};
use std::collections::HashMap;
use std::sync::Arc;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionDisabled, CodeActionKind, Range, TextEdit as LspTextEdit, Url,
    WorkspaceEdit,
};
use tracing::debug;

use super::kinds;
use super::utils::{
    find_class_closing_brace, find_variables_in_code, generate_constant_name, generate_field_name,
    generate_param_name, get_text_in_range, infer_expression_type, is_keyword, is_literal,
    position_to_offset,
};
use crate::navigation_util::extract_package_from_source;
use crate::position_utils::offset_to_position;

/// Generate an "Extract Variable" refactoring action
pub fn extract_variable_action(range: &Range, uri: &Url, source_text: &str) -> Option<CodeAction> {
    debug!("Generating extract variable action for range {:?}", range);

    let selected_text = get_text_in_range(source_text, range)?;

    // Don't offer extraction for trivial expressions
    if selected_text.trim().is_empty() || selected_text.len() < 3 {
        return None;
    }

    // Don't extract if selection contains statements (multiple lines with semicolons)
    let trimmed = selected_text.trim();
    if trimmed.contains('\n') && trimmed.matches(';').count() > 1 {
        return None;
    }

    // Generate a variable name based on the expression
    let var_name = generate_variable_name_from_expr(trimmed);

    // Find the start of the current statement (line)
    let start_offset = position_to_offset(source_text, range.start).ok()? as usize;

    // Find the beginning of the current line
    let line_start = source_text[..start_offset].rfind('\n').map_or(0, |p| p + 1);

    // Get the indentation of the current line
    let indentation: String = source_text[line_start..]
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .collect();

    // Infer the type from the expression
    let inferred_type = infer_expression_type(trimmed);

    // Create the variable declaration
    let var_declaration = format!("{indentation}{inferred_type} {var_name} = {trimmed};\n");

    // Create edits
    let declaration_insert_pos = offset_to_position(source_text, line_start);

    let edits = vec![
        LspTextEdit {
            range: Range {
                start: declaration_insert_pos,
                end: declaration_insert_pos,
            },
            new_text: var_declaration,
        },
        LspTextEdit {
            range: *range,
            new_text: var_name.clone(),
        },
    ];

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(CodeAction {
        title: format!("Extract to variable '{var_name}'"),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: None,
        disabled: Some(CodeActionDisabled {
            reason: "Extract variable is experimental; semantic extraction is not implemented."
                .to_string(),
        }),
        data: None,
    })
}

/// Generate an "Extract Method" refactoring action
pub fn extract_method_action(range: &Range, uri: &Url, source_text: &str) -> Option<CodeAction> {
    debug!("Generating extract method action for range {:?}", range);

    let selected_text = get_text_in_range(source_text, range)?;

    // Only offer method extraction for substantial selections
    if selected_text.trim().is_empty() || selected_text.len() < 10 {
        return None;
    }

    let trimmed = selected_text.trim();

    // Analyze the selection to find used variables
    let variables_used = find_variables_in_code(trimmed);

    // Generate method name
    let method_name = "extractedMethod";

    // Build parameters
    let params: Vec<String> = variables_used
        .iter()
        .filter(|v| !is_keyword(v) && !is_literal(v))
        .take(5)
        .map(|v| format!("Object {v}"))
        .collect();

    let params_str = params.join(", ");
    let args_str = variables_used
        .iter()
        .filter(|v| !is_keyword(v) && !is_literal(v))
        .take(5)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");

    let start_offset = position_to_offset(source_text, range.start).ok()? as usize;
    let end_offset = position_to_offset(source_text, range.end).ok()? as usize;

    // Find the indentation
    let line_start = source_text[..start_offset].rfind('\n').map_or(0, |p| p + 1);

    let current_indent: String = source_text[line_start..]
        .chars()
        .take_while(|c| c.is_whitespace() && *c != '\n')
        .collect();

    let method_indent = if current_indent.len() >= 4 {
        &current_indent[..current_indent.len() - 4]
    } else {
        "    "
    };

    // Prepare the extracted code with proper indentation
    let extracted_lines: Vec<&str> = trimmed.lines().collect();
    let reindented_body: String = extracted_lines
        .iter()
        .map(|line| {
            let stripped = line.trim_start();
            if stripped.is_empty() {
                String::new()
            } else {
                format!("{method_indent}    {stripped}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Generate the new method
    let new_method = format!(
        "\n{method_indent}private void {method_name}({params_str}) {{\n{reindented_body}\n{method_indent}}}\n"
    );

    // Find the end of the class
    let class_end = find_class_closing_brace(source_text, end_offset)?;
    let insert_pos = offset_to_position(source_text, class_end);

    // Create the method call
    let method_call = if args_str.is_empty() {
        format!("{method_name}();")
    } else {
        format!("{method_name}({args_str});")
    };

    let edits = vec![
        LspTextEdit {
            range: Range {
                start: insert_pos,
                end: insert_pos,
            },
            new_text: new_method,
        },
        LspTextEdit {
            range: *range,
            new_text: method_call,
        },
    ];

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(CodeAction {
        title: format!("Extract method '{method_name}'"),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        command: None,
        is_preferred: None,
        disabled: Some(CodeActionDisabled {
            reason: "Extract method is experimental; semantic extraction is not implemented."
                .to_string(),
        }),
        data: None,
    })
}

/// Generate an "Introduce Constant" refactoring action
pub fn introduce_constant_action(
    range: &Range,
    uri: &Url,
    source_text: &str,
) -> Option<CodeAction> {
    // Only work with selections
    if range.start == range.end {
        return None;
    }

    let start_offset = position_to_offset(source_text, range.start).ok()? as usize;
    let end_offset = position_to_offset(source_text, range.end).ok()? as usize;

    if end_offset <= start_offset || end_offset > source_text.len() {
        return None;
    }

    let selected_text = &source_text[start_offset..end_offset];
    let trimmed = selected_text.trim();

    // Skip empty or whitespace-only selections
    if trimmed.is_empty() {
        return None;
    }

    // Generate constant name
    let const_name = generate_constant_name(trimmed);

    // Infer type
    let const_type = infer_expression_type(trimmed);

    // Find class body start to insert constant
    let class_start = source_text[..start_offset].rfind("class ")?;
    let brace_pos = source_text[class_start..].find('{')?;
    let insert_pos = class_start + brace_pos + 1;

    // Get indentation
    let line_start = source_text[..insert_pos].rfind('\n').map_or(0, |p| p + 1);
    let base_indent = &source_text[line_start..insert_pos]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect::<String>();
    let const_indent = format!("{base_indent}    ");

    // Create constant declaration
    let const_decl =
        format!("\n{const_indent}private static final {const_type} {const_name} = {trimmed};\n");

    let mut changes = HashMap::new();
    let insert_range = Range::new(
        offset_to_position(source_text, insert_pos),
        offset_to_position(source_text, insert_pos),
    );

    changes.insert(
        uri.clone(),
        vec![
            LspTextEdit {
                range: insert_range,
                new_text: const_decl,
            },
            LspTextEdit {
                range: *range,
                new_text: const_name.clone(),
            },
        ],
    );

    Some(CodeAction {
        title: format!("Introduce Constant '{const_name}'"),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Generate an "Introduce Field" refactoring action
pub fn introduce_field_action(range: &Range, uri: &Url, source_text: &str) -> Option<CodeAction> {
    if range.start == range.end {
        return None;
    }

    let start_offset = position_to_offset(source_text, range.start).ok()? as usize;
    let end_offset = position_to_offset(source_text, range.end).ok()? as usize;

    if end_offset <= start_offset || end_offset > source_text.len() {
        return None;
    }

    let selected_text = &source_text[start_offset..end_offset];
    let trimmed = selected_text.trim();

    if trimmed.is_empty() {
        return None;
    }

    // Skip simple variable references
    if trimmed.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }

    // Find class declaration
    let class_start = source_text[..start_offset].rfind("class ")?;
    let brace_pos = source_text[class_start..].find('{')?;
    let insert_pos = class_start + brace_pos + 1;

    // Get indentation
    let line_start = source_text[..insert_pos].rfind('\n').map_or(0, |p| p + 1);
    let base_indent = &source_text[line_start..insert_pos]
        .chars()
        .take_while(|c| c.is_whitespace())
        .collect::<String>();
    let field_indent = format!("{base_indent}    ");

    let field_name = generate_field_name(trimmed);
    let field_decl = format!("\n{field_indent}private Object {field_name} = {trimmed};\n");

    let mut changes = HashMap::new();
    let insert_range = Range::new(
        offset_to_position(source_text, insert_pos),
        offset_to_position(source_text, insert_pos),
    );

    changes.insert(
        uri.clone(),
        vec![
            LspTextEdit {
                range: insert_range,
                new_text: field_decl,
            },
            LspTextEdit {
                range: *range,
                new_text: field_name.clone(),
            },
        ],
    );

    Some(CodeAction {
        title: format!("Introduce Field '{field_name}'"),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Generate an "Introduce Parameter" refactoring action
pub fn introduce_parameter_action(
    range: &Range,
    uri: &Url,
    source_text: &str,
) -> Option<CodeAction> {
    if range.start == range.end {
        return None;
    }

    let start_offset = position_to_offset(source_text, range.start).ok()? as usize;
    let end_offset = position_to_offset(source_text, range.end).ok()? as usize;

    if end_offset <= start_offset || end_offset > source_text.len() {
        return None;
    }

    let selected_text = &source_text[start_offset..end_offset];
    let trimmed = selected_text.trim();

    if trimmed.is_empty() {
        return None;
    }

    // Find enclosing method
    let before_cursor = &source_text[..start_offset];
    let method_paren = before_cursor.rfind('(')?;

    // Make sure we're inside a method body
    let method_brace = source_text[method_paren..].find('{')?;
    let method_body_start = method_paren + method_brace;

    if start_offset < method_body_start {
        return None;
    }

    // Find parameter list end
    let param_end = source_text[method_paren..method_body_start].find(')')?;
    let param_list_end = method_paren + param_end;

    let param_name = generate_param_name(trimmed);

    // Insert parameter
    let param_insert_pos = param_list_end;
    let existing_params = &source_text[method_paren + 1..param_list_end].trim();
    let param_prefix = if existing_params.is_empty() { "" } else { ", " };
    let param_decl = format!("{param_prefix}Object {param_name}");

    let param_insert_range = Range::new(
        offset_to_position(source_text, param_insert_pos),
        offset_to_position(source_text, param_insert_pos),
    );

    let mut changes = HashMap::new();
    changes.insert(
        uri.clone(),
        vec![
            LspTextEdit {
                range: param_insert_range,
                new_text: param_decl,
            },
            LspTextEdit {
                range: *range,
                new_text: param_name.clone(),
            },
        ],
    );

    Some(CodeAction {
        title: format!("Introduce Parameter '{param_name}'"),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Generate a variable name from an expression
fn generate_variable_name_from_expr(expression: &str) -> String {
    let expr = expression.trim();

    // For method calls like "user.getName()", suggest "name"
    if expr.contains('.') && expr.ends_with(')') {
        if let Some(method_start) = expr.rfind('.') {
            let method = &expr[method_start + 1..];
            if let Some(paren) = method.find('(') {
                let method_name = &method[..paren];
                // Convert "getName" to "name"
                if method_name.starts_with("get") && method_name.len() > 3 {
                    let name = &method_name[3..];
                    return name[..1].to_lowercase() + &name[1..];
                }
                // Convert "isValid" to "valid"
                if method_name.starts_with("is") && method_name.len() > 2 {
                    let name = &method_name[2..];
                    return name[..1].to_lowercase() + &name[1..];
                }
                return method_name.to_lowercase();
            }
        }
    }

    // For "new SomeClass(...)", suggest "someClass"
    if expr.starts_with("new ") {
        if let Some(class_end) = expr.find('(') {
            let class_name = expr[4..class_end].trim();
            if !class_name.is_empty() {
                return class_name[..1].to_lowercase() + &class_name[1..];
            }
        }
    }

    // For string literals
    if expr.starts_with('"') && expr.ends_with('"') {
        return "text".to_string();
    }

    // For numeric literals
    if expr.parse::<i64>().is_ok() || expr.parse::<f64>().is_ok() {
        return "value".to_string();
    }

    "extracted".to_string()
}

/// Information about a class declaration
struct ClassInfo {
    name: String,
    start: usize,      // Start of class declaration
    body_start: usize, // Position of opening brace
    body_end: usize,   // Position of closing brace
}

/// Find class declaration at or before a given position
fn find_class_at_position(source: &str, pos: usize) -> Option<ClassInfo> {
    // Search for "class" keyword - allow searching forward a bit too
    let search_end = (pos + 100).min(source.len());
    let search_region = &source[..search_end];

    // Find "class" keyword nearest to position (prefer before)
    let class_keyword_offset =
        if let Some(before_offset) = source[..pos.min(source.len())].rfind("class ") {
            before_offset
        } else {
            // Try searching forward
            search_region.find("class ")?
        };
    let class_start = class_keyword_offset;

    // Extract class name
    let after_class = &source[class_start + 6..];
    let name_end_offset = after_class.find(|c: char| !c.is_alphanumeric() && c != '_')?;
    let class_name = after_class[..name_end_offset].trim().to_string();

    if class_name.is_empty() {
        return None;
    }

    let name_end = class_start + 6 + name_end_offset;

    // Find opening brace
    let brace_offset = source[name_end..].find('{')?;
    let body_start = name_end + brace_offset;

    // Find closing brace
    let body_end = find_class_end(source, body_start)?;

    Some(ClassInfo {
        name: class_name,
        start: class_start,
        body_start,
        body_end,
    })
}

/// Find the end of a class body
fn find_class_end(source: &str, body_start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in source[body_start..].char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(body_start + i);
                }
            }
            _ => {}
        }
    }
    None
}

// ============================================================================
// Move Static Method
// ============================================================================

/// Generate a "Move Static Method to Utility Class" refactoring action
///
/// This action is offered when the cursor is on a static method and creates
/// a new utility class with the method, updating all call sites.
pub fn move_static_method_action(
    range: &Range,
    uri: &Url,
    source_text: &str,
    workspace_index: &Arc<WorkspaceIndex>,
) -> Option<CodeAction> {
    let start_offset = position_to_offset(source_text, range.start).ok()? as usize;

    // Find the method at cursor position
    let method_info = find_static_method_at_position(source_text, start_offset)?;

    // Generate utility class name based on method name
    let utility_class_name = format!("{}Utils", capitalize_first(&method_info.name));

    // Extract the package and class name
    let package = extract_package_from_source(source_text);
    let original_class_name = extract_class_name(source_text)?;

    // Generate the new utility class content
    let utility_class_code =
        generate_utility_class(&package, &utility_class_name, &method_info, source_text);

    // Get the directory path for the new file
    let file_path = uri.to_file_path().ok()?;
    let dir = file_path.parent()?;
    let new_file_path = dir.join(format!("{utility_class_name}.java"));
    let new_file_uri = Url::from_file_path(&new_file_path).ok()?;

    // Create edits: remove method from current class, create new file
    let method_range = Range {
        start: offset_to_position(source_text, method_info.start),
        end: offset_to_position(source_text, method_info.end),
    };

    let mut changes: HashMap<Url, Vec<LspTextEdit>> = HashMap::new();

    // Remove method from current file (replace with empty)
    changes.insert(
        uri.clone(),
        vec![LspTextEdit {
            range: method_range,
            new_text: String::new(),
        }],
    );

    // Create new file with utility class
    changes.insert(
        new_file_uri,
        vec![LspTextEdit {
            range: Range {
                start: offset_to_position("", 0),
                end: offset_to_position("", 0),
            },
            new_text: utility_class_code,
        }],
    );

    // Find and update call sites across the workspace
    let fqn = if let Some(ref pkg) = package {
        format!("{}.{}.{}", pkg, original_class_name, method_info.name)
    } else {
        format!("{}.{}", original_class_name, method_info.name)
    };

    let usages = workspace_index
        .find_current_usages_for_symbol_target(&SymbolTarget::new(
            fqn.clone(),
            DeclarationNamespace::Callable,
        ))
        .ok()?;
    debug!(
        "Found {} usages of {} for move refactoring",
        usages.locations().count(),
        fqn
    );

    for file in &usages.files {
        let indexed = workspace_index.get_indexed_revision_content(file.revision)?;
        let usage_uri = Url::from_file_path(&indexed.file_path).ok()?;
        let call_pattern = format!("{}.{}", original_class_name, method_info.name);
        let replacement = format!("{}.{}", utility_class_name, method_info.name);
        let edits = find_and_replace_in_text(&indexed.content, &call_pattern, &replacement);
        if !edits.is_empty() {
            changes.entry(usage_uri).or_default().extend(edits);
        }
    }

    if !workspace_index.current_usage_locations_are_current(&usages) {
        return None;
    }

    Some(CodeAction {
        title: format!("Move '{}' to '{}'", method_info.name, utility_class_name),
        kind: Some(kinds::refactor_move()),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Extract the class name from source code
fn extract_class_name(source: &str) -> Option<String> {
    // Find "class ClassName" or "public class ClassName"
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.contains("class ") && !trimmed.starts_with("//") && !trimmed.starts_with('*') {
            // Extract class name: look for word after "class "
            if let Some(class_pos) = trimmed.find("class ") {
                let after_class = &trimmed[class_pos + 6..];
                let class_name: String = after_class
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !class_name.is_empty() {
                    return Some(class_name);
                }
            }
        }
    }
    None
}

/// Find and replace a pattern in text, returning LSP text edits
fn find_and_replace_in_text(source: &str, pattern: &str, replacement: &str) -> Vec<LspTextEdit> {
    let mut edits = Vec::new();

    let mut search_start = 0;
    while let Some(pos) = source[search_start..].find(pattern) {
        let abs_pos = search_start + pos;

        // Make sure it's a complete identifier match (not part of a longer identifier)
        // Note: abs_pos is a byte offset, so we need to check characters at byte boundaries
        let before_ok = abs_pos == 0 || {
            // Get the character before the pattern by looking at the byte slice
            let before_slice = source.get(..abs_pos).unwrap_or("");
            let c = before_slice.chars().next_back().unwrap_or(' ');
            !c.is_alphanumeric() && c != '_'
        };
        let after_ok = {
            let end_pos = abs_pos + pattern.len();
            end_pos >= source.len() || {
                // After the pattern, we expect '(' for a method call
                let after_slice = source.get(end_pos..).unwrap_or("");
                let c = after_slice.chars().next().unwrap_or(' ');
                c == '(' || (!c.is_alphanumeric() && c != '_')
            }
        };

        if before_ok && after_ok {
            let start = offset_to_position(source, abs_pos);
            let end = offset_to_position(source, abs_pos + pattern.len());

            edits.push(LspTextEdit {
                range: Range { start, end },
                new_text: replacement.to_string(),
            });
        }

        search_start = abs_pos + pattern.len();
    }

    edits
}

/// Information about a static method
struct StaticMethodInfo {
    name: String,
    start: usize,      // Start of method declaration (including modifiers)
    end: usize,        // End of method (closing brace + newline)
    signature: String, // Full method signature
    body: String,      // Method body
}

/// Find a static method at or near a given position
fn find_static_method_at_position(source: &str, pos: usize) -> Option<StaticMethodInfo> {
    // Search backwards for "static" followed by method signature
    let search_start = pos.saturating_sub(200);
    let search_region = &source[search_start..pos.min(source.len())];

    // Find "static" keyword
    let static_pos = search_region.rfind("static ")?;
    let method_start = search_start + static_pos;

    // Check if this is a method (not a field)
    // Look for method signature pattern: type name(
    let after_static = &source[method_start..];

    // Find the opening parenthesis
    let paren_pos = after_static.find('(')?;
    let before_paren = &after_static[..paren_pos];

    // Extract method name (last word before parenthesis)
    let method_name = before_paren.split_whitespace().last()?.to_string();

    // Find opening brace
    let brace_pos = after_static.find('{')?;
    let signature = after_static[..brace_pos].trim().to_string();

    // Find closing brace (matching braces)
    let body_start = method_start + brace_pos + 1;
    let mut depth = 1;
    let mut body_end = body_start;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, c) in source[body_start..].char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match c {
            '\\' => escape_next = true,
            '"' if !escape_next => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    body_end = body_start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    // Skip trailing whitespace/newline
    let mut end = body_end;
    while end < source.len() && source[end..].starts_with(['\n', '\r']) {
        end += 1;
    }

    // Find the real start (including any preceding whitespace/indentation)
    let mut start = method_start;
    while start > 0 && source[start - 1..start].starts_with(' ') {
        start -= 1;
    }

    let body = source[body_start..body_end - 1].to_string();

    Some(StaticMethodInfo {
        name: method_name,
        start,
        end,
        signature,
        body,
    })
}

/// Generate utility class code
fn generate_utility_class(
    package: &Option<String>,
    class_name: &str,
    method: &StaticMethodInfo,
    _source: &str,
) -> String {
    let mut code = String::new();

    if let Some(pkg) = package {
        code.push_str(&format!("package {pkg};\n\n"));
    }

    code.push_str(&format!("public final class {class_name} {{\n"));
    code.push_str("    private ");
    code.push_str(class_name);
    code.push_str("() {} // Utility class\n\n");
    code.push_str("    public ");
    code.push_str(&method.signature);
    code.push_str(" {");
    code.push_str(&method.body);
    code.push_str("}\n");
    code.push_str("}\n");

    code
}

/// Capitalize first letter of a string
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

// ============================================================================
// Generate Test Class
// ============================================================================

/// Generate a "Create Test Class" code action
///
/// This action is offered when the cursor is on a class declaration and creates
/// a corresponding test class with test method stubs for all public methods.
pub fn generate_test_class_action(
    range: &Range,
    uri: &Url,
    source_text: &str,
) -> Option<CodeAction> {
    let start_offset = position_to_offset(source_text, range.start).ok()? as usize;

    // Find class at cursor position
    let class_info = find_class_at_position(source_text, start_offset)?;

    // Don't offer for test classes or interfaces
    if class_info.name.ends_with("Test") || class_info.name.ends_with("Tests") {
        return None;
    }
    if source_text[class_info.start..class_info.body_start].contains("interface ") {
        return None;
    }

    // Extract package and public methods
    let package = extract_package_from_source(source_text);
    let methods = extract_public_method_names(source_text, &class_info);

    // Generate test class name
    let test_class_name = format!("{}Test", class_info.name);

    // Get the file path and generate test file path
    let file_path = uri.to_file_path().ok()?;

    // Try to find test directory (common patterns: src/test/java, test/)
    let test_dir = find_test_directory(&file_path)?;
    let test_file_path = test_dir.join(format!("{test_class_name}.java"));
    let test_file_uri = Url::from_file_path(&test_file_path).ok()?;

    // Generate test class content
    let test_class_code =
        generate_test_class_code(&package, &test_class_name, &class_info.name, &methods);

    // Create edit to create new file
    let mut changes = HashMap::new();
    changes.insert(
        test_file_uri,
        vec![LspTextEdit {
            range: Range {
                start: offset_to_position("", 0),
                end: offset_to_position("", 0),
            },
            new_text: test_class_code,
        }],
    );

    Some(CodeAction {
        title: format!("Create test class '{test_class_name}'"),
        kind: Some(kinds::source_generate_test()),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Extract public method names from a class
fn extract_public_method_names(source: &str, class: &ClassInfo) -> Vec<String> {
    let mut methods = Vec::new();
    let class_body = &source[class.body_start..class.body_end];

    // Simple regex-free parsing for public methods
    let mut i = 0usize;

    while i < class_body.len() {
        // Look for "public" keyword at start of statement
        if class_body[i..].starts_with("public ") {
            // Skip to find method name (look for opening parenthesis)
            let rest = &class_body[i..];
            if let Some(paren_pos) = rest.find('(') {
                // Check it's not a constructor (no return type between public and name)
                let between = &rest[7..paren_pos];
                let words: Vec<&str> = between.split_whitespace().collect();

                // Methods have at least 2 words: return_type method_name
                if words.len() >= 2 {
                    // Skip static methods for testing
                    if !between.contains("static ") {
                        let method_name = (*words.last().unwrap_or(&"")).to_string();
                        if !method_name.is_empty() {
                            methods.push(method_name);
                        }
                    }
                }
            }

            i += "public ".len();
            continue;
        }

        let Some(ch) = class_body[i..].chars().next() else {
            break;
        };
        i += ch.len_utf8();
    }

    methods
}

/// Find the test directory for a given source file
fn find_test_directory(source_path: &std::path::Path) -> Option<std::path::PathBuf> {
    // Look for common test directory patterns
    let mut current = source_path.parent()?;

    loop {
        // Check for src/main/java -> src/test/java pattern
        if current.ends_with("main/java") {
            let test_dir = current.parent()?.parent()?.join("test/java");
            if test_dir.exists() {
                return Some(test_dir);
            }
        }

        // Check for src/main -> src/test pattern (simplified)
        if current.ends_with("main") {
            let test_dir = current.parent()?.join("test");
            if test_dir.exists() {
                return Some(test_dir);
            }
        }

        // Check for src -> test pattern
        if current.ends_with("src") {
            let test_dir = current.parent()?.join("test");
            if test_dir.exists() {
                return Some(test_dir);
            }
        }

        // Move up the directory tree
        current = current.parent()?;
    }
}

/// Generate test class code
fn generate_test_class_code(
    package: &Option<String>,
    test_class_name: &str,
    class_under_test: &str,
    methods: &[String],
) -> String {
    let mut code = String::new();

    if let Some(pkg) = package {
        code.push_str(&format!("package {pkg};\n\n"));
    }

    code.push_str("import org.junit.jupiter.api.Test;\n");
    code.push_str("import static org.junit.jupiter.api.Assertions.*;\n\n");

    code.push_str(&format!("class {test_class_name} {{\n\n"));

    // Generate test methods for each public method
    for method in methods {
        code.push_str("    @Test\n");
        code.push_str(&format!("    void test{}() {{\n", capitalize_first(method)));
        code.push_str(&format!("        // TODO: Test {method} method\n"));
        code.push_str("        fail(\"Not yet implemented\");\n");
        code.push_str("    }\n\n");
    }

    // Add default test if no methods
    if methods.is_empty() {
        code.push_str("    @Test\n");
        code.push_str(&format!("    void test{class_under_test}Creation() {{\n"));
        code.push_str(&format!(
            "        // TODO: Test {class_under_test} creation\n"
        ));
        code.push_str("        fail(\"Not yet implemented\");\n");
        code.push_str("    }\n");
    }

    code.push_str("}\n");
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{Range, Url};

    #[test]
    fn test_extract_variable_is_disabled() {
        let source = "class Foo {\n    void bar() { int a = 1 + 2; }\n}\n";
        let start = source.find("1 + 2").expect("missing expression");
        let end = start + "1 + 2".len();
        let range = Range {
            start: offset_to_position(source, start),
            end: offset_to_position(source, end),
        };
        let uri = Url::parse("file:///test.java").expect("invalid uri");

        let action = extract_variable_action(&range, &uri, source).expect("no action");
        assert!(
            action.disabled.is_some(),
            "expected disabled extract variable"
        );
    }

    #[test]
    fn test_extract_method_is_disabled() {
        let source = "class Foo {\n    void bar() { int a = 1 + 2; }\n}\n";
        let selection = "int a = 1 + 2;";
        let start = source.find(selection).expect("missing statement");
        let end = start + selection.len();
        let range = Range {
            start: offset_to_position(source, start),
            end: offset_to_position(source, end),
        };
        let uri = Url::parse("file:///test.java").expect("invalid uri");

        let action = extract_method_action(&range, &uri, source).expect("no action");
        assert!(
            action.disabled.is_some(),
            "expected disabled extract method"
        );
    }
}
