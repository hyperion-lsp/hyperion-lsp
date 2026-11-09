// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Call site analysis for method overload resolution.
//!
//! This module provides utilities for analyzing method call sites to extract
//! information needed for resolving method overloads, particularly argument counts.

use hyperion_parser::syntax::SyntaxKind;
use hyperion_parser::JavaLanguage;
use rowan::{NodeOrToken, SyntaxNode, SyntaxToken};
use tracing::debug;

/// Information about a method call site.
#[derive(Debug, Clone)]
pub struct CallSiteInfo {
    /// Name of the method being called
    pub method_name: String,
    /// Number of arguments at the call site
    pub argument_count: usize,
    /// The ARG_LIST node if found
    pub arg_list_node: Option<SyntaxNode<JavaLanguage>>,
}

/// Analyze a call site starting from a token.
///
/// Given a token (typically the method identifier), this finds the METHOD_CALL_EXPR
/// associated with the call and extracts information about the call site.
///
/// The AST structure for method calls is:
/// ```text
/// EXPR_STMT
///   NAME_EXPR
///     Token IDENT: "methodName"
///   METHOD_CALL_EXPR
///     ARG_LIST
///       Token L_PAREN: "("
///       ... arguments ...
///       Token R_PAREN: ")"
/// ```
pub fn analyze_call_site(token: &SyntaxToken<JavaLanguage>) -> Option<CallSiteInfo> {
    let method_name = token.text().to_string();

    // Find the METHOD_CALL_EXPR - it's a sibling of the NAME_EXPR containing our token
    let method_call = find_method_call_for_token(token)?;

    // Find ARG_LIST and count arguments
    let arg_list = method_call
        .children()
        .find(|n| n.kind() == SyntaxKind::ARG_LIST);

    let argument_count = arg_list.as_ref().map_or(0, count_arguments);

    debug!(
        "Call site analysis: method={}, arg_count={}",
        method_name, argument_count
    );

    Some(CallSiteInfo {
        method_name,
        argument_count,
        arg_list_node: arg_list,
    })
}

/// Find the METHOD_CALL_EXPR associated with a method name token.
///
/// A token anywhere in a call's receiver or argument list also has a
/// `METHOD_CALL_EXPR` ancestor. It is not the invoked method name. For each
/// enclosing call, select the final identifier before that call's direct
/// argument list and require it to be the supplied token.
pub fn find_method_call_for_token(
    token: &SyntaxToken<JavaLanguage>,
) -> Option<SyntaxNode<JavaLanguage>> {
    token.parent_ancestors().find(|node| {
        if node.kind() != SyntaxKind::METHOD_CALL_EXPR {
            return false;
        }
        let Some(arguments) = node
            .children()
            .find(|child| child.kind() == SyntaxKind::ARG_LIST)
        else {
            return false;
        };
        let arguments_start = arguments.text_range().start();
        let method_name = node
            .descendants_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .take_while(|candidate| candidate.text_range().start() < arguments_start)
            .filter(|candidate| candidate.kind().is_identifier())
            .last();

        method_name.is_some_and(|method_name| method_name.text_range() == token.text_range())
    })
}

/// Find the enclosing METHOD_CALL_EXPR from a token (legacy function).
pub fn find_enclosing_method_call(
    token: &SyntaxToken<JavaLanguage>,
) -> Option<SyntaxNode<JavaLanguage>> {
    find_method_call_for_token(token)
}

/// Count the number of arguments in an ARG_LIST node.
///
/// Empty ARG_LIST = 0 args
/// Single expression = 1 arg
/// N commas = N+1 args (with at least one expression)
pub fn count_arguments(arg_list: &SyntaxNode<JavaLanguage>) -> usize {
    use rowan::NodeOrToken;

    let mut count = 0;
    let mut has_expression = false;

    for child in arg_list.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) => {
                // Any node child is typically an expression argument
                // Skip certain non-expression nodes if needed
                let kind = n.kind();
                if is_expression_kind(kind) && !has_expression {
                    count = 1;
                    has_expression = true;
                }
            }
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::COMMA => {
                // Each comma means another argument follows
                count += 1;
            }
            _ => {}
        }
    }

    debug!(
        "count_arguments: has_expression={}, count={}",
        has_expression, count
    );
    count
}

/// Check if a SyntaxKind represents an expression.
const fn is_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::NAME_EXPR
            | SyntaxKind::FIELD_ACCESS_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::NEW_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::METHOD_REF_EXPR
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
    )
}

#[cfg(test)]
mod exact_call_tests {
    use hyperion_parser::{Lexer, Parser};

    use super::*;

    fn identifier_tokens(source: &str, name: &str) -> Vec<SyntaxToken<JavaLanguage>> {
        let tokens = Lexer::new(source).collect();
        let (green, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
        let root: SyntaxNode<JavaLanguage> = SyntaxNode::new_root(green);
        root.descendants_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|token| token.kind().is_identifier() && token.text() == name)
            .collect()
    }

    #[test]
    fn only_the_invoked_name_owns_an_enclosing_call() {
        let source = concat!(
            "class C { String name; Source source; ",
            "boolean test(String other) { return ",
            "this.name.equals(other) && source.make().consume(other); } }"
        );

        let name = identifier_tokens(source, "name")
            .pop()
            .expect("field access");
        assert!(find_method_call_for_token(&name).is_none());

        let other = identifier_tokens(source, "other")
            .pop()
            .expect("outer argument");
        assert!(find_method_call_for_token(&other).is_none());

        let equals = identifier_tokens(source, "equals")
            .pop()
            .expect("equals invocation");
        assert_eq!(
            find_method_call_for_token(&equals)
                .expect("equals call")
                .text(),
            "this.name.equals(other)"
        );

        let make = identifier_tokens(source, "make")
            .pop()
            .expect("make invocation");
        assert_eq!(
            find_method_call_for_token(&make)
                .expect("inner call")
                .text(),
            "source.make()"
        );

        let consume = identifier_tokens(source, "consume")
            .pop()
            .expect("consume invocation");
        assert_eq!(
            find_method_call_for_token(&consume)
                .expect("outer call")
                .text(),
            "source.make().consume(other)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::syntax::SyntaxNode as JavaSyntaxNode;
    use hyperion_parser::{Lexer, Parser};

    type JavaSyntaxToken = rowan::SyntaxToken<JavaLanguage>;

    fn find_method_token_recursive(node: &JavaSyntaxNode, name: &str) -> Option<JavaSyntaxToken> {
        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(t) => {
                    if t.kind() == SyntaxKind::IDENT && t.text() == name {
                        // Check if this is part of a method call:
                        // With nested trees, IDENT is inside NAME_EXPR which is a child of METHOD_CALL_EXPR,
                        // or IDENT is a direct child token of METHOD_CALL_EXPR (qualified call)
                        if let Some(parent) = t.parent() {
                            if parent.kind() == SyntaxKind::METHOD_CALL_EXPR {
                                return Some(t);
                            }
                            if parent.kind() == SyntaxKind::NAME_EXPR {
                                if let Some(grandparent) = parent.parent() {
                                    if grandparent.kind() == SyntaxKind::METHOD_CALL_EXPR {
                                        return Some(t);
                                    }
                                }
                            }
                        }
                    }
                }
                rowan::NodeOrToken::Node(n) => {
                    if let Some(t) = find_method_token_recursive(&n, name) {
                        return Some(t);
                    }
                }
            }
        }
        None
    }

    fn parse_and_find_method_call(source: &str, method_name: &str) -> Option<CallSiteInfo> {
        let tokens = Lexer::new(source).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = JavaSyntaxNode::new_root(green_node);

        let token = find_method_token_recursive(&root, method_name)?;
        analyze_call_site(&token)
    }

    #[test]
    fn test_no_arguments() {
        let source = r"
class Test {
    void test() {
        process();
    }
}
        ";

        let info = parse_and_find_method_call(source, "process").unwrap();
        assert_eq!(info.method_name, "process");
        assert_eq!(info.argument_count, 0);
    }

    #[test]
    fn test_single_argument() {
        let source = r#"
class Test {
    void test() {
        process("hello");
    }
}
        "#;

        let info = parse_and_find_method_call(source, "process").unwrap();
        assert_eq!(info.method_name, "process");
        assert_eq!(info.argument_count, 1);
    }

    #[test]
    fn test_multiple_arguments() {
        let source = r#"
class Test {
    void test() {
        process("hello", 42, true);
    }
}
        "#;

        let info = parse_and_find_method_call(source, "process").unwrap();
        assert_eq!(info.method_name, "process");
        assert_eq!(info.argument_count, 3);
    }

    #[test]
    fn test_nested_call_arguments() {
        let source = r"
class Test {
    void test() {
        outer(inner());
    }
}
        ";

        let info = parse_and_find_method_call(source, "outer").unwrap();
        assert_eq!(info.method_name, "outer");
        assert_eq!(info.argument_count, 1);
    }

    // NOTE: Method calls with receivers (like `service.process()`) have a different AST structure
    // involving FIELD_ACCESS_EXPR. The current implementation focuses on the simpler case
    // where the method is called directly. For the full overload resolution feature,
    // we'll use the existing chain resolver which already handles these cases.
    #[test]
    fn test_method_call_with_receiver() {
        let source = r#"
class Test {
    void test(Service service) {
        service.process("a", "b");
    }
}
        "#;

        let info = parse_and_find_method_call(source, "process").unwrap();
        assert_eq!(info.method_name, "process");
        assert_eq!(info.argument_count, 2);
    }

    #[test]
    fn test_chained_method_call() {
        let source = r#"
class Test {
    void test() {
        builder().withName("test").build();
    }
}
        "#;

        let info = parse_and_find_method_call(source, "withName").unwrap();
        assert_eq!(info.method_name, "withName");
        assert_eq!(info.argument_count, 1);
    }
}
