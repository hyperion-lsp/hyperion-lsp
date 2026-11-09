// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! AST extractors for hover information
//!
//! Functions that extract structured information (signatures, types, etc.) from syntax nodes

use hyperion_parser::{syntax::SyntaxKind, SyntaxNode};

/// Extract the declared type text from a FIELD_DECL node.
pub fn extract_field_type_from_node(node: &SyntaxNode) -> Option<String> {
    node.children()
        .find(|child| {
            matches!(
                child.kind(),
                SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
            )
        })
        .map(|node| extract_type_text(&node))
}

/// Extract method signature from a METHOD_DECL node
pub fn extract_method_signature_from_node(node: &SyntaxNode) -> Option<String> {
    let mut modifiers = Vec::new();
    let mut return_type = None;
    let mut method_name = None;
    let mut parameters = Vec::new();

    // Walk children to extract signature components
    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::PUBLIC_KW
                | SyntaxKind::PRIVATE_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::STATIC_KW
                | SyntaxKind::FINAL_KW
                | SyntaxKind::ABSTRACT_KW
                | SyntaxKind::SYNCHRONIZED_KW => {
                    modifiers.push(token.text().to_string());
                }
                SyntaxKind::IDENT if method_name.is_none() && return_type.is_some() => {
                    method_name = Some(token.text().to_string());
                }
                SyntaxKind::VOID_KW if return_type.is_none() => {
                    return_type = Some("void".to_string());
                }
                _ => {}
            },
            rowan::NodeOrToken::Node(child_node) => {
                match child_node.kind() {
                    SyntaxKind::CLASS_TYPE
                    | SyntaxKind::PRIMITIVE_TYPE
                    | SyntaxKind::ARRAY_TYPE
                        if return_type.is_none() =>
                    {
                        return_type = Some(extract_type_text(&child_node));
                    }
                    SyntaxKind::PARAM_LIST => {
                        // Extract parameters
                        for param in child_node.children() {
                            if param.kind() == SyntaxKind::PARAM {
                                if let Some(param_str) = extract_parameter_info_from_node(&param) {
                                    parameters.push(param_str);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Build signature
    let mut signature = String::new();

    // Add modifiers
    if !modifiers.is_empty() {
        signature.push_str(&modifiers.join(" "));
        signature.push(' ');
    }

    // Add return type
    if let Some(ret) = return_type {
        signature.push_str(&ret);
        signature.push(' ');
    }

    // Add method name
    {
        let name = method_name?;
        signature.push_str(&name);
    }

    // Add parameters
    signature.push('(');
    signature.push_str(&parameters.join(", "));
    signature.push(')');

    Some(signature)
}

/// Extract field information from a FIELD_DECL node
pub fn extract_field_info_from_node(node: &SyntaxNode) -> Option<String> {
    let mut modifiers = Vec::new();
    let mut field_type = None;
    let mut field_name = None;
    let mut element_type = None;
    let mut has_array_type = false;

    // First pass: check if there's an ARRAY_TYPE node (non-JLS sibling structure)
    for child in node.children() {
        if child.kind() == SyntaxKind::ARRAY_TYPE {
            has_array_type = true;
            break;
        }
    }

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::PUBLIC_KW
                | SyntaxKind::PRIVATE_KW
                | SyntaxKind::PROTECTED_KW
                | SyntaxKind::STATIC_KW
                | SyntaxKind::FINAL_KW
                | SyntaxKind::VOLATILE_KW
                | SyntaxKind::TRANSIENT_KW => {
                    modifiers.push(token.text().to_string());
                }
                SyntaxKind::IDENT
                    if field_name.is_none() && (field_type.is_some() || element_type.is_some()) =>
                {
                    field_name = Some(token.text().to_string());
                }
                _ => {}
            },
            rowan::NodeOrToken::Node(child_node) => match child_node.kind() {
                SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE => {
                    // If there's an ARRAY_TYPE sibling, save this as element type
                    // Otherwise, use it directly as field_type
                    if has_array_type {
                        if element_type.is_none() {
                            element_type = Some(extract_type_text(&child_node));
                        }
                    } else if field_type.is_none() {
                        field_type = Some(extract_type_text(&child_node));
                    }
                }
                SyntaxKind::ARRAY_TYPE
                    // Combine element type with array dimensions
                    if field_type.is_none() => {
                        let array_text = extract_type_text(&child_node);
                        if let Some(elem) = element_type.as_ref() {
                            field_type = Some(format!("{}{}", elem, array_text));
                        } else {
                            // ARRAY_TYPE without element type (shouldn't happen, but fallback)
                            field_type = Some(array_text);
                        }
                    }
                _ => {}
            },
        }
    }

    // Build field declaration
    let mut result = String::new();

    if !modifiers.is_empty() {
        result.push_str(&modifiers.join(" "));
        result.push(' ');
    }

    if let Some(ftype) = field_type {
        result.push_str(&ftype);
        result.push(' ');
    }

    {
        let name = field_name?;
        result.push_str(&name);
    }

    Some(result)
}

/// Extract variable information from a LOCAL_VAR_DECL node
pub fn extract_variable_info_from_node(node: &SyntaxNode) -> Option<String> {
    let mut var_type = None;
    let mut var_name = None;
    let mut is_final = false;

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::FINAL_KW => is_final = true,
                SyntaxKind::IDENT if var_name.is_none() && var_type.is_some() => {
                    var_name = Some(token.text().to_string());
                }
                _ => {}
            },
            rowan::NodeOrToken::Node(child_node) => match child_node.kind() {
                SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                    if var_type.is_none() =>
                {
                    var_type = Some(extract_type_text(&child_node));
                }
                _ => {}
            },
        }
    }

    // Build variable declaration
    let mut result = String::new();

    if is_final {
        result.push_str("final ");
    }

    if let Some(vtype) = var_type {
        result.push_str(&vtype);
        result.push(' ');
    }

    {
        let name = var_name?;
        result.push_str(&name);
    }

    Some(result)
}

/// Extract pattern variable information from a PATTERN_INSTANCEOF_EXPR node
/// Returns formatted string like "final BankAccount bankAccount" if the pattern variable name matches
pub fn extract_pattern_variable_info_from_node(
    node: &SyntaxNode,
    var_name: &str,
) -> Option<String> {
    // The AST structure is:
    // PATTERN_INSTANCEOF_EXPR
    //   NAME_EXPR (expression being checked)
    //   INSTANCEOF_KW
    //   TYPE_PATTERN
    //     CLASS_TYPE / PRIMITIVE_TYPE / ARRAY_TYPE
    //     IDENT (pattern variable name)

    // Find the TYPE_PATTERN child first
    let type_pattern = node
        .children()
        .find(|c| c.kind() == SyntaxKind::TYPE_PATTERN)?;

    let mut pattern_type = None;
    let mut pattern_name = None;

    // Walk through TYPE_PATTERN's children to find the type and variable name
    for child in type_pattern.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                // The variable name is an IDENT token after the type
                if token.kind() == SyntaxKind::IDENT && pattern_type.is_some() {
                    pattern_name = Some(token.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(child_node) => {
                // The type can be CLASS_TYPE, PRIMITIVE_TYPE, or ARRAY_TYPE
                if matches!(
                    child_node.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                ) {
                    pattern_type = Some(extract_type_text(&child_node));
                }
            }
        }
    }

    // Only return info if this pattern variable matches the name we're looking for
    if let Some(name) = pattern_name {
        if name == var_name {
            if let Some(ptype) = pattern_type {
                // Pattern variables are implicitly final
                return Some(format!("final {ptype} {name}"));
            }
        }
    }

    None
}

/// Extract parameter information from a PARAM node
pub fn extract_parameter_info_from_node(node: &SyntaxNode) -> Option<String> {
    let mut param_type = None;
    let mut param_name = None;
    let mut is_final = false;
    let mut is_varargs = false;

    for child in node.children_with_tokens() {
        match child {
            rowan::NodeOrToken::Token(token) => {
                let kind = token.kind();
                match kind {
                    SyntaxKind::FINAL_KW => is_final = true,
                    SyntaxKind::ELLIPSIS => is_varargs = true,
                    // Use is_identifier() to also match contextual keywords like `record`, `var`, `yield`
                    _ if kind.is_identifier() && param_name.is_none() && param_type.is_some() => {
                        param_name = Some(token.text().to_string());
                    }
                    _ => {}
                }
            }
            rowan::NodeOrToken::Node(child_node) => match child_node.kind() {
                SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
                    if param_type.is_none() =>
                {
                    param_type = Some(extract_type_text(&child_node));
                }
                _ => {}
            },
        }
    }

    // Build parameter declaration
    let mut result = String::new();

    if is_final {
        result.push_str("final ");
    }

    if let Some(ptype) = param_type {
        result.push_str(&ptype);
        if is_varargs {
            result.push_str("...");
        }
        result.push(' ');
    }

    if let Some(name) = param_name {
        result.push_str(&name);
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Extract type text from a type node (CLASS_TYPE, PRIMITIVE_TYPE, ARRAY_TYPE)
#[allow(clippy::only_used_in_recursion)]
pub fn extract_type_text(node: &SyntaxNode) -> String {
    match node.kind() {
        SyntaxKind::PRIMITIVE_TYPE => {
            // Extract primitive type keyword
            for child in node.children_with_tokens() {
                if let Some(token) = child.as_token() {
                    if matches!(
                        token.kind(),
                        SyntaxKind::VOID_KW
                            | SyntaxKind::INT_KW
                            | SyntaxKind::LONG_KW
                            | SyntaxKind::SHORT_KW
                            | SyntaxKind::BYTE_KW
                            | SyntaxKind::CHAR_KW
                            | SyntaxKind::FLOAT_KW
                            | SyntaxKind::DOUBLE_KW
                            | SyntaxKind::BOOLEAN_KW
                    ) {
                        return token.text().to_string();
                    }
                }
            }
            "unknown".to_string()
        }
        SyntaxKind::CLASS_TYPE => {
            // Extract class name and type arguments properly
            // Use children() to avoid flattening the tree (which would merge type args)
            let mut class_name = String::new();
            let mut type_args = Vec::new();

            for child in node.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Token(token) => {
                        if token.kind() == SyntaxKind::IDENT {
                            class_name.push_str(token.text());
                        } else if token.kind() == SyntaxKind::DOT {
                            class_name.push('.');
                        }
                    }
                    rowan::NodeOrToken::Node(child_node) => {
                        if child_node.kind() == SyntaxKind::TYPE_ARGS {
                            // Recursively extract type arguments
                            for type_arg in child_node.children() {
                                type_args.push(extract_type_text(&type_arg));
                            }
                        } else if child_node.kind() == SyntaxKind::QUALIFIED_NAME {
                            // Handle qualified names (e.g., java.util.Map)
                            for token in child_node.descendants_with_tokens() {
                                if let rowan::NodeOrToken::Token(t) = token {
                                    if t.kind() == SyntaxKind::IDENT {
                                        class_name.push_str(t.text());
                                    } else if t.kind() == SyntaxKind::DOT {
                                        class_name.push('.');
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Format with type arguments if present
            if type_args.is_empty() {
                class_name
            } else {
                format!("{}<{}>", class_name, type_args.join(", "))
            }
        }
        SyntaxKind::ARRAY_TYPE => {
            // Extract element type and array brackets
            let mut element_type = String::new();
            let mut bracket_count = 0;

            for child in node.children_with_tokens() {
                match child {
                    rowan::NodeOrToken::Node(child_node) => {
                        if matches!(
                            child_node.kind(),
                            SyntaxKind::CLASS_TYPE
                                | SyntaxKind::PRIMITIVE_TYPE
                                | SyntaxKind::ARRAY_TYPE
                        ) {
                            element_type = extract_type_text(&child_node);
                        }
                    }
                    rowan::NodeOrToken::Token(token) => {
                        if token.kind() == SyntaxKind::L_BRACKET {
                            bracket_count += 1;
                        }
                    }
                }
            }

            format!("{}{}", element_type, "[]".repeat(bracket_count))
        }
        _ => node.text().to_string(),
    }
}
