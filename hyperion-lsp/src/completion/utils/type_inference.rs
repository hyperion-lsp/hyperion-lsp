// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type inference utilities for smart completion
//!
//! Provides functions for inferring expected types from code context,
//! including return statements, method arguments, and assignments.

use crate::completion::type_utils;
use crate::completion::utils::text_helpers;
use crate::position_utils::position_to_offset;
use hyperion_parser::syntax::SyntaxKind;
use hyperion_semantics::db::{FileId, ParseDatabase, SemanticDatabase};
use hyperion_semantics::symbol::{PrimitiveType, SymbolTable, Type};
use std::sync::Arc;
use tower_lsp::lsp_types::Position;
use tracing::debug;

/// Infer expected type from code context for smart completion
///
/// This analyzes the code before cursor to determine what type is expected,
/// enabling type-aware completion ranking.
pub fn infer_expected_type(
    source_text: &str,
    position: Position,
    before_cursor: &str,
    file_id: FileId,
    snapshot: &salsa::Snapshot<hyperion_semantics::db::Database>,
    workspace_index: Arc<hyperion_workspace::index::WorkspaceIndex>,
) -> Option<String> {
    let trimmed = before_cursor.trim_end();

    // Pattern 1: Assignment - "Type varName = |"
    // e.g., "String name = " -> expects String
    if let Some(expected) = type_utils::infer_type_from_assignment(trimmed) {
        return Some(expected);
    }

    // The same pattern with part of the right-hand side already written:
    // "String name = customer.get|" still expects String.
    if let Some(declaration) = declaration_up_to_assignment(trimmed) {
        if let Some(expected) = type_utils::infer_type_from_assignment(declaration) {
            return Some(expected);
        }
    }

    // Pattern 2: Return statement - "return |" in a typed method
    if let Some(expected) =
        infer_type_from_return_statement(source_text, position, trimmed, file_id, snapshot)
    {
        return Some(expected);
    }

    // Pattern 3: Method argument - "methodName(|" or "methodName(arg1, |"
    if let Some(expected) =
        infer_type_from_method_argument(source_text, position, file_id, snapshot, workspace_index)
    {
        return Some(expected);
    }

    None
}

/// For a statement like `String s = a.b(` returns the text through its
/// (single, top-level) `=`, provided the cursor is still directly in the
/// right-hand side rather than inside a call's arguments.
fn declaration_up_to_assignment(before_cursor: &str) -> Option<&str> {
    let statement_start = before_cursor.rfind([';', '{', '}']).map_or(0, |i| i + 1);
    let statement = &before_cursor[statement_start..];
    let bytes = statement.as_bytes();
    let mut depth = 0i32;
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b'=' if depth == 0 => {
                let before = index.checked_sub(1).map(|i| bytes[i]);
                let after = bytes.get(index + 1).copied();
                if matches!(
                    before,
                    Some(
                        b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'&' | b'|' | b'^'
                    )
                ) || after == Some(b'=')
                {
                    return None;
                }
                let rest = &statement[index + 1..];
                let balanced = rest.matches('(').count() <= rest.matches(')').count();
                return balanced.then(|| &before_cursor[..=statement_start + index]);
            }
            _ => {}
        }
    }
    None
}

/// Infer expected type from return statement
///
/// When cursor is after "return", infer the containing method's return type
///
/// Example: In `public String getName() { return | }`, infers "String"
pub fn infer_type_from_return_statement(
    source_text: &str,
    position: Position,
    before_cursor: &str,
    file_id: FileId,
    snapshot: &salsa::Snapshot<hyperion_semantics::db::Database>,
) -> Option<String> {
    // Check if we're after "return" keyword
    let trimmed = before_cursor.trim_end();
    if !trimmed.ends_with("return") {
        return None;
    }

    // Parse the source to find containing method
    let offset = position_to_offset(source_text, position).ok()? as u32;

    // Reuse Salsa's cached parse tree instead of re-lexing + re-parsing
    let tree = snapshot.parse_file(file_id);
    let root = tree.syntax();

    // Find token at cursor
    let token = root
        .token_at_offset(rowan::TextSize::from(offset))
        .right_biased()?;

    // Walk up to find METHOD_DECL
    let mut node = token.parent()?;
    loop {
        if node.kind() == SyntaxKind::METHOD_DECL {
            break;
        }
        if node.kind() == SyntaxKind::SOURCE_FILE {
            // Reached top without finding method
            return None;
        }
        node = node.parent()?;
    }

    // Extract return type from method declaration
    // METHOD_DECL structure: [modifiers] ReturnType MethodName(params) [throws] body
    for child in node.children_with_tokens() {
        // Skip modifiers and find the type
        if let Some(type_node) = child.as_node() {
            if matches!(
                type_node.kind(),
                SyntaxKind::PRIMITIVE_TYPE
                    | SyntaxKind::CLASS_TYPE
                    | SyntaxKind::ARRAY_TYPE
                    | SyntaxKind::TYPE_PARAM
            ) {
                // Extract type text
                let type_text = type_node.text().to_string();

                // Simplify generic types: List<String> -> List
                let simple_type = type_text
                    .split('<')
                    .next()
                    .unwrap_or(&type_text)
                    .trim()
                    .to_string();

                debug!("Inferred return type from method: {}", simple_type);
                return Some(simple_type);
            }
        }
    }

    None
}

/// Infer expected type from method argument position
///
/// When cursor is inside method call parentheses, infer the parameter type
///
/// Example: In `setUser(|)`, if setUser takes User param, infers "User"
pub fn infer_type_from_method_argument(
    source_text: &str,
    position: Position,
    file_id: FileId,
    snapshot: &salsa::Snapshot<hyperion_semantics::db::Database>,
    workspace_index: Arc<hyperion_workspace::index::WorkspaceIndex>,
) -> Option<String> {
    let offset = position_to_offset(source_text, position).ok()? as u32;

    // Get parse tree and cached symbol table from Salsa
    let (root, symbol_table) = {
        let tree = snapshot.parse_file(file_id);
        let table = snapshot.file_symbol_table(file_id);
        (tree.syntax(), table)
    };

    // Find token at cursor
    let token = root
        .token_at_offset(rowan::TextSize::from(offset))
        .right_biased()?;

    // Walk up to find METHOD_CALL_EXPR or NEW_EXPR
    let mut node = token.parent()?;
    let mut method_name = None;
    let mut is_constructor = false;
    let mut receiver_expr: Option<String> = None;

    loop {
        match node.kind() {
            SyntaxKind::METHOD_CALL_EXPR => {
                // Extract method name and receiver
                let mut found_dot = false;
                let mut method_ident = None;

                for child in node.children_with_tokens() {
                    if child.kind() == SyntaxKind::DOT {
                        found_dot = true;
                    } else if child.kind() == SyntaxKind::IDENT {
                        if let Some(tok) = child.as_token() {
                            if found_dot {
                                // This is the method name (after dot)
                                method_ident = Some(tok.text().to_string());
                            } else {
                                // This might be the receiver (before dot)
                                receiver_expr = Some(tok.text().to_string());
                            }
                        }
                    } else if !found_dot {
                        // Check for expression nodes as receiver
                        if let Some(expr_node) = child.as_node() {
                            if matches!(
                                expr_node.kind(),
                                SyntaxKind::NAME_EXPR
                                    | SyntaxKind::FIELD_ACCESS_EXPR
                                    | SyntaxKind::METHOD_CALL_EXPR
                            ) {
                                receiver_expr = Some(expr_node.text().to_string());
                            }
                        }
                    }
                }
                method_name = method_ident;
                break;
            }
            SyntaxKind::NEW_EXPR => {
                // Extract class name for constructor
                for child in node.children_with_tokens() {
                    if child.kind() == SyntaxKind::CLASS_TYPE {
                        if let Some(type_node) = child.as_node() {
                            method_name = Some(type_node.text().to_string());
                            is_constructor = true;
                            break;
                        }
                    }
                }
                break;
            }
            SyntaxKind::SOURCE_FILE => {
                return None;
            }
            _ => {
                node = node.parent()?;
            }
        }
    }

    let method_name = method_name?;

    // Find the opening parenthesis and count parameters
    let paren_offset = text_helpers::find_opening_paren(&node)?;
    let param_index =
        text_helpers::count_commas_before_position(source_text, paren_offset, offset as usize);

    debug!(
        "Method argument context: method={}, receiver={:?}, is_constructor={}, param_index={}",
        method_name, receiver_expr, is_constructor, param_index
    );

    // Look up method signature from workspace
    if is_constructor {
        // For constructors, look up by class name
        lookup_constructor_param_type(&method_name, param_index, workspace_index)
    } else if let Some(receiver) = receiver_expr {
        // For method calls, resolve receiver type first using cached symbol table
        lookup_method_param_type(
            &receiver,
            &method_name,
            param_index,
            &symbol_table,
            offset,
            workspace_index,
        )
    } else {
        // No receiver - might be local method or static import
        None
    }
}

/// Look up constructor parameter type from workspace
pub fn lookup_constructor_param_type(
    class_name: &str,
    param_index: u32,
    workspace_index: Arc<hyperion_workspace::index::WorkspaceIndex>,
) -> Option<String> {
    // Try to find the class in workspace
    let fqns = workspace_index.get_fqns_by_simple_name(class_name);

    for fqn in fqns {
        // Look for constructors (method name == class name or "<init>")
        let constructor_fqn = format!("{fqn}.<init>");
        if let Some(symbol) = workspace_index.get_symbol(&constructor_fqn) {
            if let Some(param_type) = symbol.parameters.get(param_index as usize) {
                debug!(
                    "Found constructor param type: {} at index {}",
                    param_type, param_index
                );
                return Some(text_helpers::simplify_type_name(param_type));
            }
        }

        // Also check for constructor with same name
        let alt_constructor_fqn = format!("{fqn}.{class_name}");
        if let Some(symbol) = workspace_index.get_symbol(&alt_constructor_fqn) {
            if let Some(param_type) = symbol.parameters.get(param_index as usize) {
                debug!(
                    "Found constructor param type: {} at index {}",
                    param_type, param_index
                );
                return Some(text_helpers::simplify_type_name(param_type));
            }
        }
    }

    None
}

/// Look up method parameter type from workspace
///
/// Optimized to use Salsa-cached symbol table for fast lookup.
pub fn lookup_method_param_type(
    receiver: &str,
    method_name: &str,
    param_index: u32,
    symbol_table: &SymbolTable,
    offset: u32,
    workspace_index: Arc<hyperion_workspace::index::WorkspaceIndex>,
) -> Option<String> {
    // First, try to resolve receiver type
    // Simple heuristic: if receiver starts with uppercase, it's a class name (static call)
    let receiver_type = if receiver.chars().next()?.is_uppercase() {
        receiver.to_string()
    } else {
        // For instance calls, resolve the variable type using cached symbol table
        let scope = symbol_table.find_scope_at_offset(offset)?;

        // Try to resolve the receiver as a variable
        let symbol = symbol_table
            .lookup_symbol(receiver, scope)
            .and_then(|id| symbol_table.get_symbol(id))?;

        let type_fqn = type_to_fqn(&symbol.type_info).ok()?;
        debug!("Resolved receiver '{}' to type '{}'", receiver, type_fqn);
        // Extract simple name from FQN
        type_fqn.split('.').next_back()?.to_string()
    };

    // Find the class in workspace
    let fqns = workspace_index.get_fqns_by_simple_name(&receiver_type);

    let mut parameter_types = Vec::new();
    for fqn in fqns {
        // Class members are indexed by their declaring class, so this avoids the
        // descriptor-free `get_symbol("Class.method")` fallback. That fallback
        // scans the entire symbol table looking for an overload and made smart
        // completion linear in the size of a large workspace.
        let members = workspace_index.get_class_members(&fqn);
        parameter_types.extend(
            members
                .iter()
                .filter(|member| member.simple_name.as_ref() == method_name)
                .filter_map(|member| member.parameters.get(param_index as usize))
                .map(|parameter| canonical_parameter_type(parameter)),
        );
    }

    // A simple receiver name may resolve to multiple classes. Only bias
    // ranking when every matching overload across every candidate class agrees.
    if let Some(param_type) = unambiguous_type(parameter_types) {
        let param_type = text_helpers::simplify_type_name(&param_type);
        debug!(
            "Found unambiguous method param type from members: {} at index {}",
            param_type, param_index
        );
        return Some(param_type);
    }

    None
}

/// Return the type only when every applicable overload agrees.
///
/// Picking the first member of a hash set made smart ranking nondeterministic
/// and could prefer completions for the wrong overload.
fn unambiguous_type(types: impl IntoIterator<Item = String>) -> Option<String> {
    let mut types = types.into_iter();
    let first = types.next()?;
    types.all(|candidate| candidate == first).then_some(first)
}

/// Bytecode-derived parameter strings may include a synthetic name
/// (`"java.lang.Object arg0"`). Ranking needs the type, not the name.
fn canonical_parameter_type(parameter: &str) -> String {
    let parameter = parameter.trim();
    let type_part = parameter
        .rsplit_once(char::is_whitespace)
        .filter(|(_, suffix)| {
            !suffix.is_empty()
                && suffix
                    .chars()
                    .all(|character| character.is_alphanumeric() || character == '_')
        })
        .map_or(parameter, |(type_part, _)| type_part.trim_end());
    type_part.to_string()
}

/// Convert a Type to an FQN string
pub fn type_to_fqn(typ: &Type) -> Result<String, String> {
    match typ {
        Type::Class(class_type) => {
            if let Some(ref pkg) = class_type.package {
                Ok(format!("{}.{}", pkg, class_type.name))
            } else {
                Ok(class_type.name.to_string())
            }
        }
        Type::Array {
            element_type,
            dimensions: _,
        } => {
            let elem_fqn = type_to_fqn(element_type)?;
            Ok(format!("{elem_fqn}[]"))
        }
        Type::Primitive(prim) => {
            use PrimitiveType::{Boolean, Byte, Char, Double, Float, Int, Long, Short};
            let name = match prim {
                Boolean => "boolean",
                Byte => "byte",
                Short => "short",
                Int => "int",
                Long => "long",
                Char => "char",
                Float => "float",
                Double => "double",
            };
            Ok(name.to_string())
        }
        Type::TypeVariable { name, bounds: _ } => Ok(name.clone()),
        Type::Void => Ok("void".to_string()),
        Type::Unknown => Err("Cannot resolve unknown type".to_string()),
        Type::Wildcard {
            upper_bound,
            lower_bound: _,
        } => {
            // For wildcard types, try to use the upper bound if available
            if let Some(bound) = upper_bound {
                type_to_fqn(bound)
            } else {
                Ok("Object".to_string())
            }
        }
        Type::Intersection { types } => {
            // For intersection types, use the first type as a heuristic
            if let Some(first) = types.first() {
                type_to_fqn(first)
            } else {
                Err("Empty intersection type".to_string())
            }
        }
        Type::Union { types } => {
            // For union types, use the first type as a heuristic
            if let Some(first) = types.first() {
                type_to_fqn(first)
            } else {
                Err("Empty union type".to_string())
            }
        }
        Type::Null => Ok("null".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_parameter_type, unambiguous_type};

    #[test]
    fn normalizes_bytecode_parameter_names() {
        assert_eq!(
            canonical_parameter_type("java.lang.Object arg0"),
            "java.lang.Object"
        );
        assert_eq!(
            canonical_parameter_type("java.util.Map<String, Integer> values"),
            "java.util.Map<String, Integer>"
        );
        assert_eq!(
            canonical_parameter_type("java.lang.String"),
            "java.lang.String"
        );
    }

    #[test]
    fn overload_expected_type_requires_agreement() {
        assert_eq!(
            unambiguous_type(["String".to_string(), "String".to_string()]),
            Some("String".to_string())
        );
        assert_eq!(
            unambiguous_type(["String".to_string(), "Object".to_string()]),
            None
        );
        assert_eq!(unambiguous_type(Vec::<String>::new()), None);
        assert_eq!(
            unambiguous_type(["a.Token".to_string(), "b.Token".to_string()]),
            None
        );
        assert_eq!(
            unambiguous_type(["List<String>".to_string(), "List<Integer>".to_string()]),
            None
        );
    }
}
