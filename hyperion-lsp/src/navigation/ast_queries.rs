// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! AST-based queries that replace text-scanning patterns in navigation.
//!
//! Each function answers a semantic question by walking the syntax tree rather than
//! scanning raw source text with `rfind`/`find`. This avoids bugs where a text pattern
//! matches an unrelated token elsewhere in the file.

use hyperion_parser::syntax::{
    LocalVarDecl, MethodDecl, Param, SyntaxKind, SyntaxNode, SyntaxToken,
};

/// Returns `true` if the token at `offset` in a Java syntax tree is a comment or string literal.
///
/// This is the canonical implementation — all call sites should use this instead of
/// inline `matches!()` checks against individual `SyntaxKind` variants.
pub fn is_in_comment_or_string_java(syntax_root: &SyntaxNode, offset: u32) -> bool {
    let text_size = rowan::TextSize::from(offset);
    let token = match syntax_root.token_at_offset(text_size).right_biased() {
        Some(t) => t,
        None => return false,
    };
    token.kind().is_non_code()
}

/// Returns `true` if the token at `offset` in a Kotlin syntax tree is a comment or string literal.
pub fn is_in_comment_or_string_kotlin(
    syntax_root: &hyperion_kotlin::SyntaxNode,
    offset: u32,
) -> bool {
    let text_size = rowan::TextSize::from(offset);
    let token = match syntax_root.token_at_offset(text_size).right_biased() {
        Some(t) => t,
        None => return false,
    };
    token.kind().is_non_code()
}

/// Returns `true` if the next non-whitespace sibling token after `token` is `(`.
///
/// Replaces `is_method_call_after_token` which scanned raw source text past the token's
/// end offset — fragile when source text and token ranges diverged.
pub fn is_followed_by_paren(token: &SyntaxToken) -> bool {
    // First check direct siblings of the token itself
    if check_next_meaningful_is_lparen(token.next_sibling_or_token()) {
        return true;
    }
    // If the token is nested (e.g., IDENT inside NAME_EXPR inside METHOD_CALL_EXPR),
    // walk up through parents and check their siblings too
    if let Some(parent) = token.parent() {
        if check_next_meaningful_is_lparen(parent.next_sibling_or_token()) {
            return true;
        }
        // Also check children of the parent after the token position
        let token_end = token.text_range().end();
        for child in parent.children_with_tokens() {
            if child.text_range().start() >= token_end {
                match child {
                    rowan::NodeOrToken::Token(ref t) if t.kind().is_trivia() => continue,
                    rowan::NodeOrToken::Token(ref t) => return t.kind() == SyntaxKind::L_PAREN,
                    rowan::NodeOrToken::Node(ref n) => {
                        if let Some(first) = n.first_token() {
                            return first.kind() == SyntaxKind::L_PAREN;
                        }
                        return false;
                    }
                }
            }
        }
    }
    false
}

fn check_next_meaningful_is_lparen(
    start: Option<rowan::NodeOrToken<SyntaxNode, SyntaxToken>>,
) -> bool {
    let mut sibling = start;
    while let Some(s) = sibling {
        match s {
            rowan::NodeOrToken::Token(ref t) => {
                if t.kind().is_trivia() {
                    sibling = s.next_sibling_or_token();
                    continue;
                }
                return t.kind() == SyntaxKind::L_PAREN;
            }
            rowan::NodeOrToken::Node(ref n) => {
                if let Some(first) = n.first_token() {
                    return first.kind() == SyntaxKind::L_PAREN;
                }
                return false;
            }
        }
    }
    false
}

/// Returns `true` if the receiver expression preceding `token`'s dot-access is a
/// method call (i.e., ends with `)`).
///
/// Walks up to the `FIELD_ACCESS_EXPR` or `METHOD_CALL_EXPR` parent and checks
/// whether the receiver child is a `METHOD_CALL_EXPR`.
///
/// Replaces the `source_text.get(0..offset).rfind('.') + ends_with(')')` pattern in
/// `qualified_access_resolver.rs`.
pub fn is_receiver_method_call(token: &SyntaxToken) -> bool {
    for ancestor in token.parent_ancestors() {
        match ancestor.kind() {
            SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::METHOD_CALL_EXPR => {
                // The receiver is the first child expression
                if let Some(first_child) = ancestor.children().next() {
                    return first_child.kind() == SyntaxKind::METHOD_CALL_EXPR;
                }
                return false;
            }
            _ => continue,
        }
    }
    false
}

/// Finds a method declaration by name in the tree and returns its return type text
/// (including generics, e.g. `Optional<Map<String, Integer>>`).
///
/// Replaces `find_method_return_type` in `lambda_inference.rs` which extracted the
/// return type from source text using string scanning.
pub fn extract_method_return_type(root: &SyntaxNode, method_name: &str) -> Option<String> {
    for node in root.descendants() {
        if node.kind() != SyntaxKind::METHOD_DECL {
            continue;
        }
        let method = MethodDecl::new(node);
        let Some(name) = method.name() else {
            continue;
        };
        if name.text() != method_name {
            continue;
        }
        if let Some(ty) = method.return_type() {
            let text = ty.syntax().text().to_string();
            let trimmed = text.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

/// Finds a local variable or parameter declaration by name and returns its declared
/// type text (including generics).
///
/// Replaces the text-scanning portion of `find_variable_type` in `lambda_inference.rs`.
pub fn extract_var_type(root: &SyntaxNode, var_name: &str) -> Option<String> {
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::LOCAL_VAR_DECL => {
                let decl = LocalVarDecl::new(node);
                let has_var = decl.names().any(|t| t.text() == var_name);
                if !has_var {
                    continue;
                }
                if let Some(ty) = decl.ty() {
                    let text = ty.syntax().text().to_string();
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        return Some(trimmed);
                    }
                }
            }
            SyntaxKind::PARAM => {
                let param = Param::new(node);
                let has_var = param.name().is_some_and(|name| name.text() == var_name);
                if !has_var {
                    continue;
                }
                if let Some(ty) = param.ty() {
                    let text = ty.syntax().text().to_string();
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        return Some(trimmed);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Walks up the tree from `node` to find the nearest enclosing type declaration
/// (`CLASS_DECL`, `INTERFACE_DECL`, `ENUM_DECL`, `ANNOTATION_DECL`, or `RECORD_DECL`).
pub fn find_enclosing_class(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut current = Some(node.clone());
    while let Some(n) = current {
        if n.kind().is_type_declaration() {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

/// Walks up the tree from `node` to find the nearest enclosing method or constructor declaration.
pub fn find_enclosing_method(node: &SyntaxNode) -> Option<SyntaxNode> {
    let mut current = Some(node.clone());
    while let Some(n) = current {
        if matches!(
            n.kind(),
            SyntaxKind::METHOD_DECL | SyntaxKind::CONSTRUCTOR_DECL
        ) {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    fn parse_java(src: &str) -> SyntaxNode {
        let tokens = Lexer::new(src).collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "Parse errors: {errors:?}");
        SyntaxNode::new_root(green)
    }

    fn find_ident_token(root: &SyntaxNode, name: &str) -> SyntaxToken {
        root.descendants_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|t| t.kind() == SyntaxKind::IDENT && t.text() == name)
            .unwrap_or_else(|| panic!("no IDENT token '{name}' found"))
    }

    // ── is_followed_by_paren ──

    #[test]
    fn followed_by_paren_method_call() {
        let root = parse_java("class C { void test() { foo(); } }");
        let token = find_ident_token(&root, "foo");
        assert!(is_followed_by_paren(&token));
    }

    #[test]
    fn followed_by_paren_field_access() {
        let root = parse_java("class C { void test() { obj.field = 1; } }");
        let token = find_ident_token(&root, "field");
        assert!(!is_followed_by_paren(&token));
    }

    #[test]
    fn followed_by_paren_with_whitespace() {
        let root = parse_java("class C { void test() { foo (); } }");
        let token = find_ident_token(&root, "foo");
        assert!(is_followed_by_paren(&token));
    }

    // ── is_receiver_method_call ──

    #[test]
    fn receiver_is_method_call() {
        let root = parse_java("class C { void test() { getData().size(); } }");
        let token = find_ident_token(&root, "size");
        assert!(is_receiver_method_call(&token));
    }

    #[test]
    fn receiver_is_field() {
        let root = parse_java("class C { void test() { obj.size(); } }");
        let token = find_ident_token(&root, "size");
        assert!(!is_receiver_method_call(&token));
    }

    // ── extract_method_return_type ──

    #[test]
    fn method_return_type_simple() {
        let root = parse_java("class C { String getName() { return \"\"; } }");
        assert_eq!(
            extract_method_return_type(&root, "getName"),
            Some("String".to_string())
        );
    }

    #[test]
    fn method_return_type_with_generics() {
        let root =
            parse_java("class C { Optional<Map<String, Integer>> getData() { return null; } }");
        assert_eq!(
            extract_method_return_type(&root, "getData"),
            Some("Optional<Map<String, Integer>>".to_string())
        );
    }

    #[test]
    fn method_return_type_not_found() {
        let root = parse_java("class C { void test() {} }");
        assert_eq!(extract_method_return_type(&root, "nonExistent"), None);
    }

    // ── extract_var_type ──

    #[test]
    fn var_type_local() {
        let root =
            parse_java("class C { void test() { List<String> names = new ArrayList<>(); } }");
        assert_eq!(
            extract_var_type(&root, "names"),
            Some("List<String>".to_string())
        );
    }

    #[test]
    fn var_type_param() {
        let root = parse_java("class C { void test(Map<String, Integer> data) {} }");
        assert_eq!(
            extract_var_type(&root, "data"),
            Some("Map<String, Integer>".to_string())
        );
    }

    #[test]
    fn var_type_not_found() {
        let root = parse_java("class C { void test() {} }");
        assert_eq!(extract_var_type(&root, "x"), None);
    }

    #[test]
    fn var_type_simple() {
        let root = parse_java("class C { void test() { int count = 0; } }");
        assert_eq!(extract_var_type(&root, "count"), Some("int".to_string()));
    }
}
