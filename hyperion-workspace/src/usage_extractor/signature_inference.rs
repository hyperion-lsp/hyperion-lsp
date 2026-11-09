// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Shared signature inference for argument type deduction from AST nodes.
//!
//! This module provides the string-based signature inference used at **index time**
//! (workspace extractor) and **query time** (find-references). It infers simple type
//! strings (`"String"`, `"int"`, `"_"`) from argument expressions in call sites.
//!
//! # Relationship to `argument_inference.rs`
//!
//! The navigation layer has a separate, richer inference system in
//! `hyperion-lsp::navigation::argument_inference` that produces `InferredType` values
//! with confidence levels and full semantic integration. That system is used for
//! goto-definition overload resolution and operates at a different abstraction level.
//!
//! Unifying the two would require the workspace indexer to depend on
//! `hyperion-semantics::symbol::Type`, risking circular dependencies. The current
//! split is intentional: this module is fast, string-based, and dependency-free.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Build argument descriptor by finding the ARG_LIST descendant of a node.
///
/// Convenience wrapper used by constructor call extraction. Searches descendants
/// for an ARG_LIST and delegates to [`build_type_signature_from_arg_list`].
pub fn build_argument_descriptor_with_types(node: &SyntaxNode) -> String {
    for child in node.descendants() {
        if child.kind() == SyntaxKind::ARG_LIST {
            return build_type_signature_from_arg_list(&child);
        }
    }
    String::new()
}

/// Build a comma-separated type signature from an ARG_LIST node.
///
/// Each direct child node of the ARG_LIST is classified by [`infer_argument_type`].
/// Unresolvable expressions produce `"_"` (wildcard).
///
/// # Examples
///
/// - `(42, "hello")` → `"int,String"`
/// - `(x, y)` → `"_,_"`
/// - `()` → `""`
pub fn build_type_signature_from_arg_list(arg_list: &SyntaxNode) -> String {
    build_type_signature_from_arg_list_with_resolver(arg_list, |_| None)
}

/// Build type signature with a custom fallback for NAME_EXPR nodes.
///
/// The `resolve_name` closure is called for NAME_EXPR nodes before falling back
/// to `"_"`. This allows callers to inject scope-aware variable type resolution:
///
/// - **Workspace extractor** resolves parameters, locals, and fields from its
///   extraction context.
/// - **LSP find-references** passes a closure that walks the enclosing method scope
///   to resolve parameter and local variable types.
pub fn build_type_signature_from_arg_list_with_resolver<F>(
    arg_list: &SyntaxNode,
    resolve_name: F,
) -> String
where
    F: Fn(&SyntaxNode) -> Option<String>,
{
    argument_expressions(arg_list)
        .iter()
        .map(|argument| infer_argument_type_with_resolver(argument, &resolve_name))
        .collect::<Vec<_>>()
        .join(",")
}

/// Return one value-producing expression head per Java argument.
///
/// The parser represents precedence continuations as siblings of their left
/// operand. For example, `value = null` is a `NAME_EXPR` followed by an
/// `ASSIGNMENT_EXPR` within the same comma-delimited argument. Consumers must
/// therefore select the last expression before each comma rather than treating
/// every direct child as a separate argument.
#[must_use]
pub fn argument_expressions(arg_list: &SyntaxNode) -> Vec<SyntaxNode> {
    if arg_list.kind() != SyntaxKind::ARG_LIST {
        return Vec::new();
    }

    let mut arguments = Vec::new();
    let mut current_head = None;
    for element in arg_list.children_with_tokens() {
        match element {
            NodeOrToken::Node(child) => current_head = Some(child),
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => {
                if let Some(argument) = current_head.take() {
                    arguments.push(argument);
                }
            }
            _ => {}
        }
    }
    if let Some(argument) = current_head {
        arguments.push(argument);
    }
    arguments
}

/// Infer the type of an argument expression.
///
/// Returns:
/// - `"String"` for string literals
/// - `"int"` for integer literals (without L suffix)
/// - `"long"` for long literals (with L suffix)
/// - `"float"` for float literals (with f suffix)
/// - `"double"` for double literals (with d suffix or decimal point)
/// - `"boolean"` for boolean literals
/// - `"char"` for character literals
/// - `"null"` for null literal
/// - Simple class name for `new ClassName()`
/// - Array type for `new Type[]`
/// - Cast target type for `(Type) expr`
/// - `"_"` for all other expressions (variables, method calls, etc.)
pub fn infer_argument_type(arg_node: &SyntaxNode) -> String {
    infer_argument_type_with_resolver(arg_node, &|_| None)
}

fn infer_argument_type_with_resolver<F>(arg_node: &SyntaxNode, resolve_name: &F) -> String
where
    F: Fn(&SyntaxNode) -> Option<String>,
{
    // Assignment expressions have the declared type of their left operand.
    // In the precede-style tree the left operand is the preceding sibling and
    // the ASSIGNMENT_EXPR node contains the operator and right operand.
    if arg_node.kind() == SyntaxKind::ASSIGNMENT_EXPR {
        return arg_node.prev_sibling().map_or_else(
            || "_".to_string(),
            |left| infer_argument_type_with_resolver(&left, resolve_name),
        );
    }

    // Check for literal expressions
    if arg_node.kind() == SyntaxKind::LITERAL_EXPR {
        for child in arg_node.children_with_tokens() {
            if let rowan::NodeOrToken::Token(token) = child {
                return match token.kind() {
                    SyntaxKind::STRING_LITERAL | SyntaxKind::TEXT_BLOCK_LITERAL => {
                        "String".to_string()
                    }
                    SyntaxKind::INT_LITERAL => "int".to_string(),
                    SyntaxKind::LONG_LITERAL => "long".to_string(),
                    SyntaxKind::FLOAT_LITERAL => "float".to_string(),
                    SyntaxKind::DOUBLE_LITERAL => "double".to_string(),
                    SyntaxKind::BOOLEAN_LITERAL => "boolean".to_string(),
                    SyntaxKind::CHAR_LITERAL => "char".to_string(),
                    SyntaxKind::NULL_LITERAL => "null".to_string(),
                    _ => "_".to_string(),
                };
            }
        }
    }

    // Check for variable references — delegate to caller-supplied resolver
    if arg_node.kind() == SyntaxKind::NAME_EXPR {
        if let Some(ty) = resolve_name(arg_node) {
            return ty;
        }
    }

    // Check for new expressions: `new Foo()` or `new int[]`
    if arg_node.kind() == SyntaxKind::NEW_EXPR {
        for child in arg_node.children() {
            if child.kind() == SyntaxKind::CLASS_TYPE {
                let mut name_parts = Vec::new();

                for elem in child.children() {
                    if elem.kind() == SyntaxKind::QUALIFIED_NAME {
                        for token in elem.children_with_tokens() {
                            if let rowan::NodeOrToken::Token(t) = token {
                                if t.kind().is_identifier() {
                                    name_parts.push(t.text().to_string());
                                }
                            }
                        }
                    }
                }

                if name_parts.is_empty() {
                    for elem in child.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(token) = elem {
                            if token.kind().is_identifier() {
                                name_parts.push(token.text().to_string());
                            }
                        }
                    }
                }

                if !name_parts.is_empty() {
                    return name_parts.last().unwrap().clone();
                }
            }

            // Check for array types: new int[], new String[][]
            if child.kind() == SyntaxKind::ARRAY_TYPE {
                let mut element_type = None;
                let mut dimension_count = 0;

                for descendant in child.descendants_with_tokens() {
                    match descendant {
                        rowan::NodeOrToken::Node(n) => {
                            if n.kind() == SyntaxKind::PRIMITIVE_TYPE {
                                for token in n.children_with_tokens() {
                                    if let rowan::NodeOrToken::Token(t) = token {
                                        if t.kind().is_identifier()
                                            || matches!(
                                                t.kind(),
                                                SyntaxKind::INT_KW
                                                    | SyntaxKind::LONG_KW
                                                    | SyntaxKind::FLOAT_KW
                                                    | SyntaxKind::DOUBLE_KW
                                                    | SyntaxKind::BOOLEAN_KW
                                                    | SyntaxKind::CHAR_KW
                                                    | SyntaxKind::BYTE_KW
                                                    | SyntaxKind::SHORT_KW
                                            )
                                        {
                                            element_type = Some(t.text().to_string());
                                        }
                                    }
                                }
                            }
                        }
                        rowan::NodeOrToken::Token(t) => {
                            if t.kind() == SyntaxKind::L_BRACKET {
                                dimension_count += 1;
                            }
                        }
                    }
                }

                if let Some(elem_type) = element_type {
                    return format!("{}{}", elem_type, "[]".repeat(dimension_count));
                }
            }
        }
    }

    // Check for cast expressions: `(String) obj` -> "String"
    if arg_node.kind() == SyntaxKind::CAST_EXPR {
        if let Some(type_name) = extract_class_name_from_type_child(arg_node) {
            return type_name;
        }
    }

    // For all other expressions (variables, method calls, complex expressions), use placeholder
    "_".to_string()
}

/// Extract simple class name from the first CLASS_TYPE or PRIMITIVE_TYPE child of a node.
///
/// Used by both NEW_EXPR and CAST_EXPR handling in argument type inference.
pub fn extract_class_name_from_type_child(node: &SyntaxNode) -> Option<String> {
    for child in node.children() {
        if child.kind() == SyntaxKind::CLASS_TYPE {
            let mut name_parts = Vec::new();
            for elem in child.children() {
                if elem.kind() == SyntaxKind::QUALIFIED_NAME {
                    for token in elem.children_with_tokens() {
                        if let rowan::NodeOrToken::Token(t) = token {
                            if t.kind().is_identifier() {
                                name_parts.push(t.text().to_string());
                            }
                        }
                    }
                }
            }
            if name_parts.is_empty() {
                for elem in child.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(token) = elem {
                        if token.kind().is_identifier() {
                            name_parts.push(token.text().to_string());
                        }
                    }
                }
            }
            if !name_parts.is_empty() {
                return Some(name_parts.last().unwrap().clone());
            }
        }
        if child.kind() == SyntaxKind::PRIMITIVE_TYPE {
            for token in child.children_with_tokens() {
                if let rowan::NodeOrToken::Token(t) = token {
                    if t.kind().is_keyword() || t.kind().is_identifier() {
                        return Some(t.text().to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{argument_expressions, build_type_signature_from_arg_list_with_resolver};
    use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};

    #[test]
    fn assignment_continuations_form_one_argument_and_keep_lhs_type() {
        let source = r#"
            class Test {
                void test() { consume(value, state = null); }
            }
        "#;
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green);
        let arguments = root
            .descendants()
            .find(|node| {
                node.kind() == SyntaxKind::ARG_LIST && node.text().to_string().contains("value")
            })
            .expect("call arguments");

        let heads = argument_expressions(&arguments);
        assert_eq!(heads.len(), 2);
        assert_eq!(heads[0].kind(), SyntaxKind::NAME_EXPR);
        assert_eq!(heads[1].kind(), SyntaxKind::ASSIGNMENT_EXPR);
        assert_eq!(
            build_type_signature_from_arg_list_with_resolver(&arguments, |name| {
                match name.text().to_string().trim() {
                    "value" => Some("Integer".to_string()),
                    "state" => Some("Object".to_string()),
                    _ => None,
                }
            }),
            "Integer,Object"
        );
    }
}
