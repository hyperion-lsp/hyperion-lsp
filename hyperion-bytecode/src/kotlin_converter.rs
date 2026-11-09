// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin CST conversion for the bounded evaluator producer.
//!
//! This converter intentionally accepts only the expression subset that
//! `KotlinBytecodeProducer` can execute. It must never invent a value for an
//! unsupported Kotlin construct or discard a conversion error.

use crate::java_compiler::{BinaryOp, Expression, Statement, UnaryOp};
use crate::producer::{BytecodeError, BytecodeResult};
use hyperion_kotlin::{SyntaxKind, SyntaxNode};

/// Convert the single evaluator function in a parsed Kotlin source file.
pub fn convert_kotlin_source(green_node: rowan::GreenNode) -> BytecodeResult<Vec<Statement>> {
    let root = SyntaxNode::new_root(green_node);
    let function = root
        .children()
        .find(|node| node.kind() == SyntaxKind::FUNCTION_DECL)
        .ok_or_else(|| unsupported("missing top-level evaluator function"))?;

    for child in function.children() {
        if child.kind() == SyntaxKind::BLOCK {
            return convert_block(&child);
        }
        if is_expression_node(&child) {
            return Ok(vec![Statement::Return(Some(convert_expression(&child)?))]);
        }
    }

    Err(unsupported("evaluator function without a body"))
}

fn convert_block(block: &SyntaxNode) -> BytecodeResult<Vec<Statement>> {
    let mut statements = Vec::new();
    for child in block.children() {
        match child.kind() {
            SyntaxKind::RETURN_EXPR => statements.push(convert_return(&child)?),
            kind => return Err(unsupported(format!("Kotlin evaluator statement {kind:?}"))),
        }
    }

    if statements.len() > 1 {
        return Err(unsupported(
            "multiple Kotlin evaluator statements or unreachable code",
        ));
    }
    Ok(statements)
}

fn convert_return(node: &SyntaxNode) -> BytecodeResult<Statement> {
    match node.children().next() {
        Some(expression) => Ok(Statement::Return(Some(convert_expression(&expression)?))),
        None => Ok(Statement::Return(None)),
    }
}

fn is_expression_node(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::BINARY_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::UNARY_EXPR
    )
}

fn convert_expression(node: &SyntaxNode) -> BytecodeResult<Expression> {
    match node.kind() {
        SyntaxKind::LITERAL_EXPR => convert_literal(node),
        SyntaxKind::BINARY_EXPR => convert_binary(node),
        SyntaxKind::PAREN_EXPR => node
            .children()
            .find(is_expression_node)
            .ok_or_else(|| unsupported("empty parenthesized Kotlin expression"))
            .and_then(|inner| convert_expression(&inner)),
        SyntaxKind::UNARY_EXPR => convert_unary(node),
        kind => Err(unsupported(format!("Kotlin evaluator expression {kind:?}"))),
    }
}

fn convert_literal(node: &SyntaxNode) -> BytecodeResult<Expression> {
    let token = node
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind().is_literal() || token.kind() == SyntaxKind::NULL_KW)
        .ok_or_else(|| unsupported("literal expression without a value"))?;

    match token.kind() {
        SyntaxKind::INT_LITERAL => {
            let text = token.text();
            let value = text
                .parse()
                .map_err(|_| unsupported(format!("integer literal {text:?}")))?;
            Ok(Expression::IntLiteral(value))
        }
        SyntaxKind::STRING_LITERAL => {
            let text = token.text();
            if !text.starts_with('"')
                || !text.ends_with('"')
                || text.len() < 2
                || text[1..text.len() - 1].contains(['\\', '$'])
            {
                return Err(unsupported(format!("string literal {text:?}")));
            }
            Ok(Expression::StringLiteral(
                text[1..text.len() - 1].to_string(),
            ))
        }
        SyntaxKind::TRUE_KW => Ok(Expression::BooleanLiteral(true)),
        SyntaxKind::FALSE_KW => Ok(Expression::BooleanLiteral(false)),
        SyntaxKind::NULL_KW => Ok(Expression::NullLiteral),
        kind => Err(unsupported(format!("literal kind {kind:?}"))),
    }
}

fn convert_binary(node: &SyntaxNode) -> BytecodeResult<Expression> {
    let mut expressions = node.children().filter(is_expression_node);
    let left = expressions
        .next()
        .ok_or_else(|| unsupported("binary expression without a left operand"))?;
    let right = expressions
        .next()
        .ok_or_else(|| unsupported("binary expression without a right operand"))?;
    if expressions.next().is_some() {
        return Err(unsupported("binary expression with too many operands"));
    }

    let operator = node
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .find_map(|token| match token.kind() {
            SyntaxKind::PLUS => Some(BinaryOp::Add),
            SyntaxKind::MINUS => Some(BinaryOp::Subtract),
            SyntaxKind::STAR => Some(BinaryOp::Multiply),
            SyntaxKind::SLASH => Some(BinaryOp::Divide),
            _ => None,
        })
        .ok_or_else(|| unsupported("non-arithmetic Kotlin binary operator"))?;

    Ok(Expression::BinaryExpr {
        left: Box::new(convert_expression(&left)?),
        op: operator,
        right: Box::new(convert_expression(&right)?),
    })
}

fn convert_unary(node: &SyntaxNode) -> BytecodeResult<Expression> {
    let operand = node
        .children()
        .find(is_expression_node)
        .ok_or_else(|| unsupported("unary expression without an operand"))?;
    let operator = node
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .find_map(|token| match token.kind() {
            SyntaxKind::MINUS => Some(UnaryOp::Negate),
            SyntaxKind::BANG => Some(UnaryOp::LogicalNot),
            _ => None,
        })
        .ok_or_else(|| unsupported("Kotlin unary operator"))?;

    Ok(Expression::UnaryExpr {
        op: operator,
        operand: Box::new(convert_expression(&operand)?),
    })
}

fn unsupported(feature: impl Into<String>) -> BytecodeError {
    BytecodeError::UnsupportedFeature {
        feature: feature.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(source: &str) -> BytecodeResult<Vec<Statement>> {
        let parsed = hyperion_kotlin::parse(source);
        assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
        convert_kotlin_source(parsed.green_node)
    }

    #[test]
    fn accepts_the_supported_expression_tree() {
        let statements = convert("fun __eval__(): Any? { return -10 + 5 * 2 }").unwrap();
        assert_eq!(statements.len(), 1);
        assert!(matches!(statements[0], Statement::Return(Some(_))));
    }

    #[test]
    fn rejects_statements_and_calls_instead_of_fabricating_values() {
        let statement = convert("fun __eval__() { val value = 1 }").unwrap_err();
        assert!(
            statement.to_string().contains("PROPERTY_DECL"),
            "{statement}"
        );

        let call = convert("fun __eval__(): Any? { return println(1) }").unwrap_err();
        assert!(call.to_string().contains("CALL_EXPR"), "{call}");
    }
}
