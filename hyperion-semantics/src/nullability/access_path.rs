// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Stable property/index access paths used by flow-sensitive narrowing.

use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use rowan::NodeOrToken;

/// Return a stable access path such as `request.user.profile`.
///
/// Calls and other effectful expressions are deliberately excluded. Array
/// access is stable only when its index is a literal or simple name.
pub fn stable_access_path(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::NAME_EXPR | SyntaxKind::IDENT => direct_identifier(node),
        SyntaxKind::THIS_EXPR => Some("this".to_string()),
        SyntaxKind::SUPER_EXPR => Some("super".to_string()),
        SyntaxKind::FIELD_ACCESS_EXPR => {
            let receiver = node.children().next()?;
            let receiver_path = stable_access_path(&receiver)?;
            let member = node
                .children_with_tokens()
                .filter_map(NodeOrToken::into_token)
                .filter(|token| token.kind() == SyntaxKind::IDENT)
                .last()
                .map(|token| token.text().to_string())
                .or_else(|| {
                    node.descendants_with_tokens()
                        .filter_map(NodeOrToken::into_token)
                        .filter(|token| token.kind() == SyntaxKind::IDENT)
                        .last()
                        .map(|token| token.text().to_string())
                })?;
            Some(format!("{receiver_path}.{member}"))
        }
        SyntaxKind::ARRAY_ACCESS_EXPR => {
            let mut expressions = node.children().filter(|child| is_expression(child.kind()));
            let receiver = expressions.next()?;
            let index = expressions.next()?;
            if !matches!(
                index.kind(),
                SyntaxKind::NAME_EXPR | SyntaxKind::LITERAL_EXPR
            ) {
                return None;
            }
            let receiver_path = stable_access_path(&receiver)?;
            let index_text: String = index
                .text()
                .to_string()
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            Some(format!("{receiver_path}[{index_text}]"))
        }
        SyntaxKind::PAREN_EXPR => node.children().find_map(|child| stable_access_path(&child)),
        SyntaxKind::CAST_EXPR => node
            .children()
            .filter(|child| is_expression(child.kind()))
            .last()
            .and_then(|child| stable_access_path(&child)),
        _ => None,
    }
}

fn direct_identifier(node: &SyntaxNode) -> Option<String> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| token.kind() == SyntaxKind::IDENT)
        .map(|token| token.text().to_string())
}

const fn is_expression(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::CAST_EXPR
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{lexer::Lexer, parser::Parser};

    #[test]
    fn extracts_dotted_and_indexed_paths() {
        let source = "class T { void f(Request request, int i) { request.user.run(); request.items[i].run(); } }";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "{errors:?}");
        let root = SyntaxNode::new_root(green);
        let paths: Vec<String> = root
            .descendants()
            .filter(|node| {
                matches!(
                    node.kind(),
                    SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::ARRAY_ACCESS_EXPR
                )
            })
            .filter_map(|node| stable_access_path(&node))
            .collect();
        assert!(paths.iter().any(|path| path == "request.user"), "{paths:?}");
        assert!(
            paths.iter().any(|path| path == "request.items[i]"),
            "{paths:?}"
        );
    }

    #[test]
    fn casts_preserve_their_operand_path() {
        let source = "class T { void f(Object value) { synchronized ((CharSequence) value) {} } }";
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "{errors:?}");
        let root = SyntaxNode::new_root(green);
        let cast = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::CAST_EXPR)
            .expect("cast expression");
        assert_eq!(stable_access_path(&cast).as_deref(), Some("value"));
    }
}
