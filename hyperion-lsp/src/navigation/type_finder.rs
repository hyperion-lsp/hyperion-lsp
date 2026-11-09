// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Type and variable finding utilities

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_semantics::db::FileId;
use hyperion_workspace::WorkspaceIndex;
use rowan::SyntaxToken;
use std::sync::Arc;
use tower_lsp::lsp_types::{Location, Range, Url};
use tracing::debug;

use super::utils::offset_to_position;

/// Find the type of a variable at the given token position (simple name only)
pub fn find_variable_type(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    var_name: &str,
) -> Option<String> {
    find_variable_type_impl(token, var_name, false, None, None, None)
}

/// Find the type of a variable INCLUDING generic type arguments
/// For "Supplier<User> supplier", returns "Supplier<User>"
pub fn find_variable_type_with_generics(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    var_name: &str,
) -> Option<String> {
    find_variable_type_impl(token, var_name, true, None, None, None)
}

/// Find the type of a variable INCLUDING generic type arguments, with workspace for var inference
/// This version can resolve `var` declarations by using the resolver
pub fn find_variable_type_with_generics_and_workspace(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    var_name: &str,
    workspace_index: Arc<WorkspaceIndex>,
    file_id: FileId,
    root: &SyntaxNode,
) -> Option<String> {
    find_variable_type_impl(
        token,
        var_name,
        true,
        Some(workspace_index),
        Some(file_id),
        Some(root),
    )
}

/// Implementation for variable type finding
fn find_variable_type_impl(
    token: &SyntaxToken<hyperion_parser::JavaLanguage>,
    var_name: &str,
    include_generics: bool,
    workspace_index: Option<Arc<WorkspaceIndex>>,
    file_id: Option<FileId>,
    root: Option<&SyntaxNode>,
) -> Option<String> {
    let extract_fn = if include_generics {
        extract_type_with_arguments
    } else {
        extract_simple_type_name
    };
    // Find enclosing method or constructor
    let mut current = token.parent()?;
    loop {
        match current.kind() {
            SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL => {
                break;
            }
            _ => {
                current = current.parent()?;
            }
        }
    }

    // Search for variable declaration in method parameters
    // First find the PARAM_LIST node
    for child in current.children() {
        if child.kind() == SyntaxKind::PARAM_LIST {
            // Now check each parameter
            for param in child.children() {
                if param.kind() == SyntaxKind::PARAM {
                    // Check if this is our parameter
                    let has_matching_name = param.descendants_with_tokens().any(|n| {
                        if let Some(token) = n.as_token() {
                            token.kind() == SyntaxKind::IDENT && token.text() == var_name
                        } else {
                            false
                        }
                    });

                    if has_matching_name {
                        // Extract type from parameter
                        for child in param.children() {
                            if matches!(
                                child.kind(),
                                SyntaxKind::CLASS_TYPE
                                    | SyntaxKind::PRIMITIVE_TYPE
                                    | SyntaxKind::ARRAY_TYPE
                            ) {
                                return Some(extract_fn(&child));
                            }
                        }
                    }
                }
            }
        }
    }

    // Search for local variable declaration in method body
    let reference_offset = token.text_range().start();
    for child in current.children() {
        if child.kind() == SyntaxKind::BLOCK_STMT {
            if let Some(var_type) = find_variable_in_block_impl(
                &child,
                var_name,
                reference_offset,
                include_generics,
                workspace_index.clone(),
                file_id,
                root,
            ) {
                return Some(var_type);
            }
        }
    }

    // Search for field declaration in enclosing class
    // Walk up to find the containing class
    let mut class_node = current.parent();
    while let Some(node) = class_node {
        if node.kind().is_type_declaration() {
            // Search for field with matching name
            for field in node.descendants() {
                if field.kind() == SyntaxKind::FIELD_DECL {
                    let has_matching_name = field.descendants_with_tokens().any(|n| {
                        if let Some(token) = n.as_token() {
                            token.kind() == SyntaxKind::IDENT && token.text() == var_name
                        } else {
                            false
                        }
                    });

                    if has_matching_name {
                        // Extract type from field
                        for child in field.children() {
                            if matches!(
                                child.kind(),
                                SyntaxKind::CLASS_TYPE
                                    | SyntaxKind::PRIMITIVE_TYPE
                                    | SyntaxKind::ARRAY_TYPE
                            ) {
                                return Some(extract_fn(&child));
                            }
                        }
                    }
                }
            }
            break;
        }
        class_node = node.parent();
    }

    None
}

/// Recursively search for a variable declaration in a block
pub fn find_variable_in_block(
    block: &SyntaxNode,
    var_name: &str,
    reference_offset: rowan::TextSize,
) -> Option<String> {
    find_variable_in_block_impl(block, var_name, reference_offset, false, None, None, None)
}

/// Recursively search for a variable declaration in a block (with generics)
pub fn find_variable_in_block_with_generics(
    block: &SyntaxNode,
    var_name: &str,
    reference_offset: rowan::TextSize,
) -> Option<String> {
    find_variable_in_block_impl(block, var_name, reference_offset, true, None, None, None)
}

fn find_variable_in_block_impl(
    block: &SyntaxNode,
    var_name: &str,
    reference_offset: rowan::TextSize,
    include_generics: bool,
    workspace_index: Option<Arc<WorkspaceIndex>>,
    file_id: Option<FileId>,
    root: Option<&SyntaxNode>,
) -> Option<String> {
    let extract_fn = if include_generics {
        extract_type_with_arguments
    } else {
        extract_simple_type_name
    };

    for child in block.descendants() {
        // Only look at declarations before the reference
        if child.text_range().start() >= reference_offset {
            break;
        }

        if child.kind() == SyntaxKind::LOCAL_VAR_DECL {
            // Extract variable name: it's the first IDENT that comes after the type
            let mut found_type = false;
            let mut is_var_type = false;
            let mut var_name_token: Option<String> = None;

            for item in child.children_with_tokens() {
                match item {
                    rowan::NodeOrToken::Node(n) => {
                        if matches!(
                            n.kind(),
                            SyntaxKind::CLASS_TYPE
                                | SyntaxKind::PRIMITIVE_TYPE
                                | SyntaxKind::ARRAY_TYPE
                        ) {
                            found_type = true;
                        } else if n.kind() == SyntaxKind::VAR_TYPE {
                            found_type = true;
                            is_var_type = true;
                        }
                    }
                    rowan::NodeOrToken::Token(t) => {
                        if found_type && t.kind() == SyntaxKind::IDENT && var_name_token.is_none() {
                            var_name_token = Some(t.text().to_string());
                            break; // Found the variable name, stop looking
                        }
                    }
                }
            }

            // Check if this is our variable
            if let Some(ref found_var_name) = var_name_token {
                if found_var_name == var_name {
                    // Handle var declarations by inferring type from initializer
                    if is_var_type {
                        if let (Some(ws), Some(fid), Some(r)) =
                            (workspace_index.as_ref(), file_id, root)
                        {
                            debug!(
                                "TYPE_FINDER: Found var declaration for '{}', inferring type",
                                var_name
                            );
                            if let Some(inferred_type) =
                                infer_var_type_from_decl(&child, ws.clone(), fid, r)
                            {
                                debug!(
                                    "TYPE_FINDER: Inferred type for '{}': {}",
                                    var_name, inferred_type
                                );
                                return Some(inferred_type);
                            }
                        }
                        debug!(
                            "TYPE_FINDER: Could not infer type for var '{}' (no workspace context)",
                            var_name
                        );
                        return None;
                    }

                    // Extract type for explicit type declarations
                    for type_child in child.children() {
                        if matches!(
                            type_child.kind(),
                            SyntaxKind::CLASS_TYPE
                                | SyntaxKind::PRIMITIVE_TYPE
                                | SyntaxKind::ARRAY_TYPE
                        ) {
                            return Some(extract_fn(&type_child));
                        }
                    }
                }
            }
        }
    }

    None
}

/// Infer the type of a var declaration using the resolver
fn infer_var_type_from_decl(
    decl_node: &SyntaxNode,
    workspace_index: Arc<WorkspaceIndex>,
    file_id: FileId,
    root: &SyntaxNode,
) -> Option<String> {
    use hyperion_semantics::resolver::ResolverBuilder;

    // Create a resolver builder with workspace context and build the symbol table
    // This is needed because var type inference may need to look up other variables
    // (e.g., var x = y.get() needs to know the type of y)
    let builder = ResolverBuilder::with_workspace(file_id, workspace_index);
    let resolver = builder.build_from_syntax(root);

    // Extract the variable name from the declaration
    let var_name = decl_node.children_with_tokens().find_map(|item| {
        if let rowan::NodeOrToken::Token(t) = item {
            if t.kind() == SyntaxKind::IDENT {
                return Some(t.text().to_string());
            }
        }
        None
    })?;

    debug!("TYPE_FINDER: Looking up type for var '{}'", var_name);

    // Look up the declaration in the scope that owns its exact source range.
    let declaration_offset = u32::from(decl_node.text_range().start());
    let result = resolver
        .find_scope_at_offset(declaration_offset)
        .and_then(|scope| {
            resolver.resolve_in_scope_at_offset(&var_name, scope, declaration_offset)
        });
    if let Some(result) = result {
        if let Some(symbol) = resolver.symbol_table().get_symbol(result.symbol_id) {
            debug!(
                "TYPE_FINDER: Found symbol '{}' with type {:?}",
                var_name, symbol.type_info
            );
            return type_to_string(&symbol.type_info);
        }
    }

    debug!(
        "TYPE_FINDER: Could not find var '{}' in symbol table",
        var_name
    );
    None
}

/// Convert a Type to a string representation (including generic type arguments)
fn type_to_string(ty: &hyperion_semantics::symbol::Type) -> Option<String> {
    use hyperion_semantics::symbol::{PrimitiveType, Type};

    match ty {
        Type::Class(ct) => {
            let mut result = ct.name.to_string();

            if !ct.type_arguments.is_empty() {
                result.push('<');
                let args: Vec<String> = ct
                    .type_arguments
                    .iter()
                    .filter_map(type_to_string)
                    .collect();
                result.push_str(&args.join(", "));
                result.push('>');
            }

            Some(result)
        }
        Type::Primitive(prim) => {
            let name = match prim {
                PrimitiveType::Boolean => "boolean",
                PrimitiveType::Byte => "byte",
                PrimitiveType::Char => "char",
                PrimitiveType::Short => "short",
                PrimitiveType::Int => "int",
                PrimitiveType::Long => "long",
                PrimitiveType::Float => "float",
                PrimitiveType::Double => "double",
            };
            Some(name.to_string())
        }
        Type::Array {
            element_type,
            dimensions,
        } => {
            let elem = type_to_string(element_type)?;
            let brackets: String = (0..*dimensions).map(|_| "[]").collect();
            Some(format!("{elem}{brackets}"))
        }
        Type::Null => Some("null".to_string()),
        Type::Unknown => None,
        Type::Void => Some("void".to_string()),
        Type::TypeVariable { name, .. } => Some(name.clone()),
        Type::Wildcard { upper_bound, .. } => {
            if let Some(b) = upper_bound {
                type_to_string(b)
            } else {
                Some("?".to_string())
            }
        }
        Type::Intersection { types } => {
            let parts: Vec<String> = types.iter().filter_map(type_to_string).collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" & "))
            }
        }
        Type::Union { types } => {
            let parts: Vec<String> = types.iter().filter_map(type_to_string).collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" | "))
            }
        }
    }
}

/// Extract simple type name from a type node (just the class name, no package)
pub fn extract_simple_type_name(node: &SyntaxNode) -> String {
    // For CLASS_TYPE, extract the identifier(s) but stop at type parameters
    if node.kind() == SyntaxKind::CLASS_TYPE {
        // Get identifiers before any TYPE_ARGUMENTS node
        // For "Pair<String, Integer>", we want just "Pair"
        // For "com.example.User", we want "User"
        for child in node.children() {
            // Stop if we hit TYPE_ARGS (generic type parameters)
            if child.kind() == SyntaxKind::TYPE_ARGS {
                break;
            }
            // CLASS_TYPE has QUALIFIED_NAME as a child containing the type name
            if child.kind() == SyntaxKind::QUALIFIED_NAME {
                // Get the last IDENT in the qualified name (the simple name)
                let mut last_ident: Option<String> = None;
                for token in child.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = token {
                        if t.kind() == SyntaxKind::IDENT {
                            last_ident = Some(t.text().to_string());
                        }
                    }
                }
                if let Some(text) = last_ident {
                    return text;
                }
            }
        }
    }

    // For PRIMITIVE_TYPE and ARRAY_TYPE, just return the text
    node.text().to_string()
}

/// Extract full type name from a type node INCLUDING generic type arguments
/// For "Supplier<User>", returns "Supplier<User>"
/// For "Map<String, Integer>", returns "Map<String, Integer>"
pub fn extract_type_with_arguments(node: &SyntaxNode) -> String {
    if node.kind() == SyntaxKind::CLASS_TYPE {
        let mut result = String::new();
        let mut type_args_str = String::new();

        for child in node.children() {
            if child.kind() == SyntaxKind::QUALIFIED_NAME {
                // Build the qualified name (e.g., "java.util.List" or just "List")
                for token in child.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(t) = token {
                        match t.kind() {
                            SyntaxKind::IDENT => result.push_str(t.text()),
                            SyntaxKind::DOT => result.push('.'),
                            _ => {}
                        }
                    }
                }
            } else if child.kind() == SyntaxKind::TYPE_ARGS {
                // Include the type arguments
                type_args_str = extract_type_args(&child);
            }
        }

        // Append type arguments if any
        result.push_str(&type_args_str);

        if !result.is_empty() {
            return result;
        }
    }

    // For PRIMITIVE_TYPE and ARRAY_TYPE, just return the text
    node.text().to_string().trim().to_string()
}

/// Extract type arguments as a string (e.g., "<String, Integer>")
fn extract_type_args(node: &SyntaxNode) -> String {
    let mut result = String::from("<");
    let mut first = true;

    for child in node.children() {
        if matches!(
            child.kind(),
            SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::WILDCARD_TYPE
        ) {
            if !first {
                result.push_str(", ");
            }
            first = false;
            result.push_str(&extract_type_with_arguments(&child));
        }
    }

    result.push('>');
    result
}

/// Find a method in the given AST (for this::method references)
pub fn find_method_in_ast(
    root: &SyntaxNode,
    method_name: &str,
    source_text: &str,
    uri: &Url,
) -> Option<Location> {
    // Find all METHOD_DECL nodes
    for node in root.descendants() {
        if node.kind() == SyntaxKind::METHOD_DECL {
            // Extract method name
            let mut return_type_found = false;
            let mut found_method_name = None;
            let mut name_token_range = None;

            for token_or_node in node.children_with_tokens() {
                match token_or_node {
                    rowan::NodeOrToken::Node(child_node) => {
                        if matches!(
                            child_node.kind(),
                            SyntaxKind::CLASS_TYPE
                                | SyntaxKind::PRIMITIVE_TYPE
                                | SyntaxKind::ARRAY_TYPE
                        ) {
                            return_type_found = true;
                        }
                    }
                    rowan::NodeOrToken::Token(token) => {
                        if token.kind() == SyntaxKind::VOID_KW {
                            return_type_found = true;
                        } else if token.kind() == SyntaxKind::IDENT
                            && return_type_found
                            && found_method_name.is_none()
                        {
                            found_method_name = Some(token.text().to_string());
                            name_token_range = Some(token.text_range());
                            break;
                        }
                    }
                }
            }

            // Check if this is the method we're looking for
            if let Some(name) = found_method_name {
                if name == method_name {
                    if let Some(range) = name_token_range {
                        let start_offset: usize = range.start().into();
                        let end_offset: usize = range.end().into();
                        let start_pos = offset_to_position(source_text, start_offset);
                        let end_pos = offset_to_position(source_text, end_offset);

                        return Some(Location {
                            uri: uri.clone(),
                            range: Range::new(start_pos, end_pos),
                        });
                    }
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{lexer::Lexer, parser::Parser, syntax::SyntaxNode};

    #[test]
    fn test_extract_simple_type_name_qualified() {
        // Regression test: extract_simple_type_name should return just "ArrayList"
        // from "java.util.ArrayList", not "java" or the full qualified name
        let code = r"
            class Test {
                java.util.ArrayList list;
            }
        ";
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        // Find CLASS_TYPE node
        fn find_class_type(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::CLASS_TYPE {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_class_type(&child) {
                    return Some(found);
                }
            }
            None
        }

        let class_type = find_class_type(&root).expect("Should find CLASS_TYPE");
        let simple_name = extract_simple_type_name(&class_type);

        assert_eq!(
            simple_name, "ArrayList",
            "Should extract simple name 'ArrayList' from qualified type"
        );
    }

    #[test]
    fn test_extract_simple_type_name_simple() {
        // Simple class name should also work
        let code = r"
            class Test {
                String name;
            }
        ";
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        fn find_class_type(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::CLASS_TYPE {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_class_type(&child) {
                    return Some(found);
                }
            }
            None
        }

        let class_type = find_class_type(&root).expect("Should find CLASS_TYPE");
        let simple_name = extract_simple_type_name(&class_type);

        assert_eq!(simple_name, "String", "Should extract simple name 'String'");
    }

    #[test]
    fn test_extract_type_with_arguments_qualified() {
        // Regression test: extract_type_with_arguments should return full qualified name with generics
        let code = r"
            class Test {
                java.util.Map<String, Integer> map;
            }
        ";
        let tokens: Vec<_> = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        fn find_class_type(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::CLASS_TYPE {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_class_type(&child) {
                    return Some(found);
                }
            }
            None
        }

        let class_type = find_class_type(&root).expect("Should find CLASS_TYPE");
        let full_type = extract_type_with_arguments(&class_type);

        assert_eq!(
            full_type, "java.util.Map<String, Integer>",
            "Should extract full type with package and generics"
        );
    }
}
