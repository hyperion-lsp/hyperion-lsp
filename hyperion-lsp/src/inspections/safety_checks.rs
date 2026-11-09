// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Safety-related inspections: resource leaks, null safety, and threading issues.

use crate::position_utils::text_range_to_lsp_range;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_workspace::import_resolver::ImportResolver;
use hyperion_workspace::{ImportMap, WorkspaceIndex};
use std::collections::HashSet;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

// ============================================================================
// Resource Leak Inspection
// ============================================================================

/// Check for resource leaks - Closeable/AutoCloseable not in try-with-resources
pub(crate) fn check_resource_leaks(
    syntax: &SyntaxNode,
    source_text: &str,
    workspace_index: &WorkspaceIndex,
    imports: Option<&ImportMap>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let Some(imports) = imports else {
        return diagnostics;
    };
    let resolver = ImportResolver::new(imports, workspace_index);

    for node in syntax.descendants() {
        if node.kind() != SyntaxKind::LOCAL_VAR_DECL {
            continue;
        }

        // Check if this declaration is inside a try-with-resources
        if is_inside_try_with_resources(&node) {
            continue;
        }

        // Extract type name from the declaration
        let type_name = extract_type_name(&node);
        let Some(type_name) = type_name else {
            continue;
        };

        let Some(type_fqn) = resolve_type_fqn(&type_name, &resolver, workspace_index) else {
            continue;
        };

        if !implements_auto_closeable(workspace_index, &type_fqn) {
            continue;
        }

        // Extract variable name for the diagnostic message
        let var_name = extract_var_name(&node);
        let var_name = var_name.as_deref().unwrap_or("resource");
        let display_type = simple_type_name(&type_fqn);

        // Check if there's a try-finally with close() nearby
        // (simple heuristic: check if the containing method has try-finally)
        if has_try_finally_with_close(syntax, var_name) {
            continue;
        }

        // Report the resource leak
        let range = node.text_range();
        let lsp_range = text_range_to_lsp_range(range, source_text);

        diagnostics.push(Diagnostic {
            range: lsp_range,
            severity: Some(DiagnosticSeverity::WARNING),
            code: Some(NumberOrString::String("resource_leak".to_string())),
            code_description: None,
            source: Some("hyperion-lint".to_string()),
            message: format!(
                "Resource '{var_name}' of type '{display_type}' should be managed with try-with-resources"
            ),
            related_information: None,
            tags: None,
            data: None,
        });
    }

    diagnostics
}

/// Check if a node is inside a try-with-resources RESOURCE_SPEC
fn is_inside_try_with_resources(node: &SyntaxNode) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        // Check for TRY_STMT which is the actual node created by the parser
        if parent.kind() == SyntaxKind::TRY_STMT {
            // Check if there's a RESOURCE_SPEC, and if our node is inside it
            for child in parent.children() {
                if child.kind() == SyntaxKind::RESOURCE_SPEC {
                    // Found a RESOURCE_SPEC, check if our node is inside it
                    if child.text_range().contains_range(node.text_range()) {
                        return true;
                    }
                }
            }
        }
        current = parent.parent();
    }
    false
}

/// Extract the type name from a LOCAL_VAR_DECL
fn extract_type_name(node: &SyntaxNode) -> Option<String> {
    for child in node.children_with_tokens() {
        match child.kind() {
            SyntaxKind::CLASS_TYPE => {
                // The CLASS_TYPE may contain QUALIFIED_NAME or direct IDENTs
                if let Some(child_node) = child.as_node() {
                    return extract_qualified_name_from_type(child_node);
                }
            }
            SyntaxKind::ARRAY_TYPE => {
                // For arrays, get the element type
                if let Some(child_node) = child.as_node() {
                    for inner in child_node.children() {
                        if inner.kind() == SyntaxKind::CLASS_TYPE {
                            return extract_qualified_name_from_type(&inner);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract the qualified (or simple) name from a CLASS_TYPE node
fn extract_qualified_name_from_type(type_node: &SyntaxNode) -> Option<String> {
    // First check for QUALIFIED_NAME
    for child in type_node.children() {
        if child.kind() == SyntaxKind::QUALIFIED_NAME {
            let mut qualified = String::new();
            for token in child.descendants_with_tokens() {
                if let Some(t) = token.as_token() {
                    if matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::DOT) {
                        qualified.push_str(t.text());
                    }
                }
            }
            if !qualified.is_empty() {
                return Some(qualified);
            }
        }
    }

    // Fallback: look for direct IDENT
    for token in type_node.children_with_tokens() {
        if token.kind() == SyntaxKind::IDENT {
            if let Some(t) = token.as_token() {
                return Some(t.text().to_string());
            }
        }
    }

    None
}

/// Extract variable name from LOCAL_VAR_DECL
fn extract_var_name(node: &SyntaxNode) -> Option<String> {
    let mut found_type = false;
    for child in node.children_with_tokens() {
        match child.kind() {
            SyntaxKind::CLASS_TYPE
            | SyntaxKind::PRIMITIVE_TYPE
            | SyntaxKind::ARRAY_TYPE
            | SyntaxKind::VAR_KW => {
                found_type = true;
            }
            SyntaxKind::EQ => break,
            SyntaxKind::IDENT if found_type => {
                if let Some(token) = child.as_token() {
                    return Some(token.text().to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// Resolve a type name to a single FQN using imports/workspace context.
fn resolve_type_fqn(
    type_name: &str,
    resolver: &ImportResolver<'_>,
    workspace_index: &WorkspaceIndex,
) -> Option<String> {
    let candidates = resolver.resolve(type_name);
    let mut matches = Vec::new();

    for candidate in candidates {
        if workspace_index.get_symbol(&candidate).is_some() {
            matches.push(candidate);
        }
    }

    if matches.len() == 1 {
        return matches.pop();
    }

    None
}

fn implements_auto_closeable(workspace_index: &WorkspaceIndex, type_fqn: &str) -> bool {
    let mut to_visit = vec![type_fqn.to_string()];
    let mut visited = HashSet::new();

    while let Some(current) = to_visit.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }

        if current == "java.lang.AutoCloseable" || current == "java.io.Closeable" {
            return true;
        }

        // Recorded symbol edges retain source spellings (`AutoCloseable`,
        // imported aliases, and so on). Walking them directly silently loses
        // package/import context and can also assign an interface the
        // superclass role. The workspace hierarchy API resolves both name and
        // role against the declaration's exact import context and fails closed
        // on missing or ambiguous targets.
        to_visit.extend(workspace_index.direct_supertypes(&current));
    }

    false
}

fn simple_type_name(type_fqn: &str) -> String {
    type_fqn.rsplit('.').next().unwrap_or(type_fqn).to_string()
}

/// Check if the containing scope has a try-finally block that closes the variable
fn has_try_finally_with_close(syntax: &SyntaxNode, var_name: &str) -> bool {
    // Look for TRY_STMT nodes that have a FINALLY_CLAUSE
    for node in syntax.descendants() {
        if node.kind() == SyntaxKind::TRY_STMT {
            // Check if it has a finally clause
            let has_finally = node
                .children()
                .any(|c| c.kind() == SyntaxKind::FINALLY_CLAUSE);
            if !has_finally {
                continue;
            }

            // Check if the finally clause calls close() on our variable
            for finally in node
                .children()
                .filter(|c| c.kind() == SyntaxKind::FINALLY_CLAUSE)
            {
                let finally_text = finally.text().to_string();
                // Simple heuristic: check if the text contains "varName.close()"
                if finally_text.contains(&format!("{var_name}.close()")) {
                    return true;
                }
                // Also check for IOUtils.closeQuietly or similar
                if finally_text.contains(var_name)
                    && (finally_text.contains("closeQuietly")
                        || finally_text.contains("close(")
                        || finally_text.contains("IOUtils"))
                {
                    return true;
                }
            }
        }
    }
    false
}

// ============================================================================
// Null Safety Inspection
// ============================================================================

/// Methods that may return null (heuristic-based detection)
const NULLABLE_METHODS: &[&str] = &[
    // Map/Collection methods
    "get",
    "remove",
    "putIfAbsent",
    "replace",
    // Optional unwrapping that can return null
    "orElse",
    "orElseGet",
    // Stream terminal operations
    "findFirst",
    "findAny",
    "max",
    "min",
    "reduce",
    // Reflection/parsing
    "forName",
    "getMethod",
    "getField",
    "getResource",
    "parse",
    // I/O that can return null
    "readLine",
    "getProperty",
    "getenv",
];

/// Check for potential null pointer dereference issues
pub(crate) fn check_null_safety(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Track variables that might be null
    let mut potentially_null_vars: HashSet<String> = HashSet::new();

    for node in syntax.descendants() {
        // Check for explicit null assignments
        if node.kind() == SyntaxKind::LOCAL_VAR_DECL {
            if let Some((var_name, is_null)) = check_null_assignment(&node) {
                if is_null {
                    potentially_null_vars.insert(var_name);
                }
            }
        }

        // Check for assignments from nullable methods
        if node.kind() == SyntaxKind::LOCAL_VAR_DECL || node.kind() == SyntaxKind::ASSIGNMENT_EXPR {
            if let Some((var_name, method_name)) = extract_assignment_from_method(&node) {
                if NULLABLE_METHODS.iter().any(|&m| method_name == m) {
                    potentially_null_vars.insert(var_name);
                }
            }
        }

        // Check for method calls on potentially null variables without null check
        if node.kind() == SyntaxKind::METHOD_CALL_EXPR {
            if let Some(receiver_name) = extract_method_call_receiver(&node) {
                if potentially_null_vars.contains(&receiver_name) {
                    // Check if there's a null check before this
                    if !has_null_check_before(&node, &receiver_name, syntax) {
                        let range = node.text_range();
                        let lsp_range = text_range_to_lsp_range(range, source_text);

                        diagnostics.push(Diagnostic {
                            range: lsp_range,
                            severity: Some(DiagnosticSeverity::WARNING),
                            code: Some(NumberOrString::String(
                                "potential_null_pointer".to_string(),
                            )),
                            code_description: None,
                            source: Some("hyperion-lint".to_string()),
                            message: format!(
                                "Potential null pointer: '{receiver_name}' may be null"
                            ),
                            related_information: None,
                            tags: None,
                            data: None,
                        });

                        // Only report once per variable
                        potentially_null_vars.remove(&receiver_name);
                    }
                }
            }
        }

        // Check for method chains on nullable results
        // Pattern: obj.get(...).something() or similar where get() may return null
        // Check at statement/declaration level to see the full chain
        if node.kind() == SyntaxKind::LOCAL_VAR_DECL || node.kind() == SyntaxKind::EXPR_STMT {
            if let Some(chain_issue) = check_method_chain_null_safety(&node, source_text) {
                diagnostics.push(chain_issue);
            }
        }
    }

    diagnostics
}

/// Check if a variable declaration assigns null
fn check_null_assignment(node: &SyntaxNode) -> Option<(String, bool)> {
    let mut var_name = None;
    let mut is_null = false;
    let mut found_type = false;

    for child in node.children_with_tokens() {
        match child.kind() {
            SyntaxKind::CLASS_TYPE | SyntaxKind::ARRAY_TYPE => {
                found_type = true;
            }
            SyntaxKind::IDENT if found_type && var_name.is_none() => {
                if let Some(token) = child.as_token() {
                    var_name = Some(token.text().to_string());
                }
            }
            SyntaxKind::NULL_LITERAL => {
                is_null = true;
            }
            _ => {}
        }
    }

    var_name.map(|name| (name, is_null))
}

/// Extract variable name and method name from an assignment
fn extract_assignment_from_method(node: &SyntaxNode) -> Option<(String, String)> {
    let text = node.text().to_string();

    // Look for pattern: varName = expr.methodName(...)
    let eq_pos = text.find('=')?;
    let before_eq = text[..eq_pos].trim();
    let after_eq = text[eq_pos + 1..].trim();

    // Get variable name (last word before =)
    let var_name = before_eq.split_whitespace().last()?.to_string();

    // Get method name (look for .methodName( pattern)
    if let Some(dot_pos) = after_eq.rfind('.') {
        let after_dot = &after_eq[dot_pos + 1..];
        if let Some(paren_pos) = after_dot.find('(') {
            let method_name = after_dot[..paren_pos].trim().to_string();
            if !method_name.is_empty() {
                return Some((var_name, method_name));
            }
        }
    }

    None
}

/// Extract the receiver name from a method call expression
fn extract_method_call_receiver(node: &SyntaxNode) -> Option<String> {
    // METHOD_CALL_EXPR only contains the arguments ()
    // We need to look at the parent (EXPR_STMT) or the full expression context
    let text = if let Some(parent) = node.parent() {
        // Check if parent contains a useful text (EXPR_STMT has the full call)
        let parent_text = parent.text().to_string();
        if parent_text.contains('.') {
            parent_text
        } else {
            node.text().to_string()
        }
    } else {
        node.text().to_string()
    };

    // Look for pattern: receiver.methodName(
    let dot_pos = text.find('.')?;
    let receiver = text[..dot_pos].trim();

    // Make sure it's a simple identifier (strip any leading whitespace or line characters)
    let receiver = receiver.lines().last().unwrap_or(receiver).trim();

    if receiver.chars().all(|c| c.is_alphanumeric() || c == '_') && !receiver.is_empty() {
        return Some(receiver.to_string());
    }

    None
}

/// Check if there's a null check before the given node
fn has_null_check_before(node: &SyntaxNode, var_name: &str, syntax: &SyntaxNode) -> bool {
    // Look for if (varName != null) or if (varName == null) patterns before this node
    let node_start = node.text_range().start();

    for if_node in syntax.descendants() {
        if if_node.kind() != SyntaxKind::IF_STMT {
            continue;
        }

        let if_start = if_node.text_range().start();
        let if_end = if_node.text_range().end();

        // Check if this if statement contains our node
        if if_start < node_start && if_end > node_start {
            let if_text = if_node.text().to_string();

            // Check for null check patterns
            if if_text.contains(&format!("{var_name} != null"))
                || if_text.contains(&format!("{var_name} == null"))
                || if_text.contains(&format!("null != {var_name}"))
                || if_text.contains(&format!("null == {var_name}"))
                || if_text.contains(&format!("Objects.nonNull({var_name})"))
                || if_text.contains(&format!("Objects.isNull({var_name})"))
            {
                return true;
            }
        }
    }

    false
}

/// Check method chains for potential null safety issues
fn check_method_chain_null_safety(node: &SyntaxNode, source_text: &str) -> Option<Diagnostic> {
    let text = node.text().to_string();

    // Look for patterns like .get(...).something or .findFirst().something
    for nullable_method in NULLABLE_METHODS {
        let pattern = format!(".{nullable_method}(");
        if let Some(pos) = text.find(&pattern) {
            // Check if there's another method call or field access after this
            let after_method = &text[pos + pattern.len()..];

            // Find the closing paren
            let mut depth = 1;
            let mut close_pos = 0;
            for (i, c) in after_method.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close_pos = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }

            // Check what comes after the closing paren
            if close_pos > 0 && close_pos + 1 < after_method.len() {
                let after_close = after_method[close_pos + 1..].trim_start();
                if after_close.starts_with('.') {
                    // There's a method call on something that might be null
                    let range = node.text_range();
                    let lsp_range = text_range_to_lsp_range(range, source_text);

                    return Some(Diagnostic {
                        range: lsp_range,
                        severity: Some(DiagnosticSeverity::WARNING),
                        code: Some(NumberOrString::String(
                            "unchecked_nullable_chain".to_string(),
                        )),
                        code_description: None,
                        source: Some("hyperion-lint".to_string()),
                        message: format!(
                            "Method chain on potentially null result of '{nullable_method}'"
                        ),
                        related_information: None,
                        tags: None,
                        data: None,
                    });
                }
            }
        }
    }

    None
}

// ============================================================================
// Threading Inspection
// ============================================================================

/// Check for common threading issues
pub(crate) fn check_threading_issues(syntax: &SyntaxNode, source_text: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for node in syntax.descendants() {
        // Check for Thread.sleep() in synchronized blocks
        if node.kind() == SyntaxKind::SYNCHRONIZED_STMT {
            if let Some(d) = check_sleep_in_synchronized(&node, source_text) {
                diagnostics.push(d);
            }
        }

        // Check for wait/notify outside synchronized
        if node.kind() == SyntaxKind::EXPR_STMT {
            if let Some(d) = check_wait_notify_outside_synchronized(&node, source_text) {
                diagnostics.push(d);
            }
        }

        // Check for synchronized on non-final fields
        if node.kind() == SyntaxKind::SYNCHRONIZED_STMT {
            if let Some(d) = check_synchronized_on_non_final(&node, syntax, source_text) {
                diagnostics.push(d);
            }
        }
    }

    diagnostics
}

fn check_sleep_in_synchronized(node: &SyntaxNode, source_text: &str) -> Option<Diagnostic> {
    // Look for Thread.sleep() calls inside the synchronized block
    for descendant in node.descendants() {
        let text = descendant.text().to_string();
        if text.contains("Thread.sleep") || text.contains("sleep(") {
            // Check if it's a method call
            if descendant.kind() == SyntaxKind::METHOD_CALL_EXPR
                || descendant.kind() == SyntaxKind::EXPR_STMT
            {
                let range = descendant.text_range();
                let lsp_range = text_range_to_lsp_range(range, source_text);

                return Some(Diagnostic {
                    range: lsp_range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: Some(NumberOrString::String(
                        "sleep_in_synchronized".to_string(),
                    )),
                    code_description: None,
                    source: Some("hyperion-lint".to_string()),
                    message: "Thread.sleep() called while holding a lock - use wait() or a scheduling mechanism instead".to_string(),
                    related_information: None,
                    tags: None,
                    data: None,
                });
            }
        }
    }
    None
}

fn check_wait_notify_outside_synchronized(
    node: &SyntaxNode,
    source_text: &str,
) -> Option<Diagnostic> {
    let text = node.text().to_string();

    // Check for wait/notify/notifyAll calls
    let is_wait_notify =
        text.contains(".wait(") || text.contains(".notify(") || text.contains(".notifyAll(");

    if !is_wait_notify {
        return None;
    }

    // Check if we're inside a synchronized block/method
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == SyntaxKind::SYNCHRONIZED_STMT {
            return None; // Inside synchronized - OK
        }
        // Check if parent method is synchronized
        if parent.kind() == SyntaxKind::METHOD_DECL {
            // Check modifier list for synchronized keyword
            let mut has_sync = false;
            let mut sib = parent.prev_sibling();
            while let Some(s) = sib {
                if s.kind() == SyntaxKind::MODIFIER_LIST {
                    let mod_text = s.text().to_string();
                    if mod_text.contains("synchronized") {
                        has_sync = true;
                    }
                }
                sib = s.prev_sibling();
            }
            if has_sync {
                return None;
            }
            break;
        }
        current = parent.parent();
    }

    // wait/notify outside synchronized
    let range = node.text_range();
    let lsp_range = text_range_to_lsp_range(range, source_text);

    Some(Diagnostic {
        range: lsp_range,
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String(
            "wait_notify_without_sync".to_string(),
        )),
        code_description: None,
        source: Some("hyperion-lint".to_string()),
        message: "wait()/notify() should be called within synchronized context".to_string(),
        related_information: None,
        tags: None,
        data: None,
    })
}

/// Check for synchronized on non-final fields
fn check_synchronized_on_non_final(
    node: &SyntaxNode,
    syntax: &SyntaxNode,
    source_text: &str,
) -> Option<Diagnostic> {
    let node_text = node.text().to_string();

    // Extract the lock expression from synchronized(expr)
    let lock_start = node_text.find('(')? + 1;
    let lock_end = node_text.find(')')?;
    let lock_expr = node_text[lock_start..lock_end].trim();

    // Skip if it's "this" or a class literal (Java class literal, not file extension)
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    if lock_expr == "this" || lock_expr.ends_with(".class") {
        return None;
    }

    // Check if the lock expression is a field and if it's final
    for field_node in syntax.descendants() {
        if field_node.kind() == SyntaxKind::FIELD_DECL {
            let field_text = field_node.text().to_string();

            // Check if this field matches the lock expression
            if !field_text.contains(lock_expr) {
                continue;
            }

            // FIELD_DECL doesn't include modifiers - look at source text line
            let field_start = field_node.text_range().start().into();
            let line_start = source_text[..field_start].rfind('\n').map_or(0, |p| p + 1);
            let full_line = &source_text[line_start..];
            let line_end = full_line.find('\n').unwrap_or(full_line.len());
            let full_line = &full_line[..line_end];

            let is_final = full_line.split_whitespace().any(|w| w == "final");

            if !is_final {
                let range = node.text_range();
                let lsp_range = text_range_to_lsp_range(range, source_text);

                return Some(Diagnostic {
                    range: lsp_range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    code: Some(NumberOrString::String("sync_on_non_final".to_string())),
                    code_description: None,
                    source: Some("hyperion-lint".to_string()),
                    message: format!(
                        "Synchronized on non-final field '{lock_expr}' - field may be reassigned"
                    ),
                    related_information: None,
                    tags: None,
                    data: None,
                });
            }
        }
    }

    None
}
