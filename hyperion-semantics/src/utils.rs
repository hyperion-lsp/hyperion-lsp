// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

/// Utility functions for semantic analysis
///
/// Re-exports utility functions from hyperion_parser for convenience
///
/// # Deprecated Functions
/// The functions re-exported here are deprecated in favor of inherent methods on `SyntaxKind`:
/// - `is_identifier_token(token)` → use `token.kind().is_identifier()`
/// - `is_identifier_kind(kind)` → use `kind.is_identifier()`
#[allow(deprecated)]
pub use hyperion_parser::utils::{is_identifier_kind, is_identifier_token};

use hyperion_parser::{SyntaxKind, SyntaxNode};
use hyperion_workspace::ImportMap;
use rowan::{NodeOrToken, TextRange};

/// Build an ImportMap from the syntax tree of a Java source file.
///
/// Extracts the package declaration, explicit imports, wildcard imports,
/// and static imports from the root syntax node.
pub fn build_imports_from_syntax(root: &SyntaxNode) -> ImportMap {
    let mut imports = ImportMap::new();

    for child in root.children() {
        match child.kind() {
            SyntaxKind::PACKAGE_DECL => {
                let mut parts = Vec::new();
                for elem in child.descendants_with_tokens() {
                    if let NodeOrToken::Token(t) = elem {
                        if t.kind().is_identifier() {
                            parts.push(t.text().to_string());
                        }
                    }
                }
                if !parts.is_empty() {
                    imports.current_package = Some(parts.join("."));
                }
            }
            SyntaxKind::IMPORT_DECL => {
                let mut is_static = false;
                let mut is_wildcard = false;
                let mut path_parts = Vec::new();

                for elem in child.descendants_with_tokens() {
                    if let NodeOrToken::Token(t) = elem {
                        match t.kind() {
                            SyntaxKind::STATIC_KW => is_static = true,
                            SyntaxKind::STAR => is_wildcard = true,
                            kind if kind.is_identifier() => path_parts.push(t.text().to_string()),
                            _ => {}
                        }
                    }
                }

                if path_parts.is_empty() {
                    continue;
                }

                let full_path = path_parts.join(".");

                if is_static {
                    if is_wildcard {
                        imports.add_static_wildcard(full_path);
                    } else if let Some(simple_name) = path_parts.last() {
                        imports.add_static_import(simple_name.clone(), full_path);
                    }
                } else if is_wildcard {
                    imports.add_wildcard(full_path);
                } else if let Some(simple_name) = path_parts.last() {
                    imports.add_import(simple_name.clone(), full_path);
                }
            }
            _ => {}
        }
    }

    imports
}

/// Extract the simple name and its text range from a CLASS_TYPE node.
pub fn extract_class_type_simple_name(node: &SyntaxNode) -> Option<(String, TextRange)> {
    let mut last_ident: Option<(String, TextRange)> = None;
    for elem in node.descendants_with_tokens() {
        if let Some(token) = elem.as_token() {
            if token.kind() == SyntaxKind::LT {
                break;
            }
            if token.kind().is_type_identifier() {
                last_ident = Some((token.text().to_string(), token.text_range()));
            }
        }
    }
    last_ident
}

/// Extract the identifier name and its text range from a NAME_EXPR node.
pub fn extract_name_expr_ident(node: &SyntaxNode) -> Option<(String, TextRange)> {
    for elem in node.descendants_with_tokens() {
        if let Some(token) = elem.as_token() {
            if token.kind().is_identifier() {
                return Some((token.text().to_string(), token.text_range()));
            }
        }
    }
    None
}

/// Strip generic type parameters from a type name.
///
/// Examples:
/// - "List<String>" → "List"
/// - "java.util.Map<K, V>" → "java.util.Map"
pub fn strip_generics(type_name: &str) -> String {
    type_name
        .split('<')
        .next()
        .unwrap_or(type_name)
        .trim()
        .to_string()
}

/// Find a case-mismatch suggestion from imports.
pub fn find_case_mismatch_suggestion(simple_name: &str, imports: &ImportMap) -> Option<String> {
    let simple_lower = simple_name.to_ascii_lowercase();
    imports
        .explicit
        .keys()
        .find(|name| name.as_str() != simple_name && name.to_ascii_lowercase() == simple_lower)
        .cloned()
}

/// Identifier tokens of every declarator in a variable declaration.
///
/// `int i = 0, len = xs.length;` declares two variables; both the field and the
/// local-variable node carry them as direct IDENT tokens, the first after the
/// type and each further one after a top-level comma (commas inside
/// initializers are nested in expression nodes and never seen here). Reading
/// only the first name left `len` undeclared, which made every use of it an
/// "unresolved symbol" in real code.
pub fn declarator_identifiers(
    declaration: &SyntaxNode,
) -> Vec<rowan::SyntaxToken<hyperion_parser::JavaLanguage>> {
    let mut names = Vec::new();
    let mut seen_type = false;
    let mut expecting_name = false;
    for element in declaration.children_with_tokens() {
        match element {
            rowan::NodeOrToken::Node(child) => {
                if matches!(
                    child.kind(),
                    SyntaxKind::CLASS_TYPE
                        | SyntaxKind::PRIMITIVE_TYPE
                        | SyntaxKind::VAR_TYPE
                        | SyntaxKind::ARRAY_TYPE
                ) && !seen_type
                {
                    seen_type = true;
                    expecting_name = true;
                }
            }
            rowan::NodeOrToken::Token(token) => {
                if !seen_type {
                    continue;
                }
                if token.kind() == SyntaxKind::COMMA {
                    expecting_name = true;
                } else if expecting_name && token.kind().is_identifier() {
                    names.push(token);
                    expecting_name = false;
                }
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::extract_class_type_simple_name;
    use hyperion_parser::syntax::SyntaxKind;
    use hyperion_parser::{Lexer, Parser, SyntaxNode};

    #[test]
    fn generic_class_types_record_each_identifier_at_its_own_range() {
        let source = "class Test { java.util.List<ConsumerGroupPartitionAssignor> value; }";
        let tokens = Lexer::new(source).collect();
        let (green, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green);

        let resolved_tokens: Vec<_> = root
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::CLASS_TYPE)
            .filter_map(|node| extract_class_type_simple_name(&node))
            .map(|(name, range)| {
                (
                    name,
                    &source[usize::from(range.start())..usize::from(range.end())],
                )
            })
            .collect();

        assert!(resolved_tokens.contains(&("List".to_string(), "List")));
        assert!(resolved_tokens.contains(&(
            "ConsumerGroupPartitionAssignor".to_string(),
            "ConsumerGroupPartitionAssignor"
        )));
    }
}
