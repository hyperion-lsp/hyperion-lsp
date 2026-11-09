// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Candidate-specific Java lambda result inference.

use super::expressions::{infer_expression, is_expression_kind, lub_two_types};
use super::TypeInferenceEngine;
use crate::call_resolution::LambdaReturnEvidence;
use crate::resolver::SymbolResolver;
use crate::symbol::Type;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};

struct LambdaParameterScope<'a>(&'a TypeInferenceEngine);

impl Drop for LambdaParameterScope<'_> {
    fn drop(&mut self) {
        self.0.lambda_parameters.borrow_mut().pop();
    }
}

pub(super) fn explicit_parameter_type(
    engine: &TypeInferenceEngine,
    parameter: &SyntaxNode,
) -> Option<Type> {
    parameter
        .children()
        .any(|child| {
            matches!(
                child.kind(),
                SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE
            )
        })
        .then(|| {
            engine
                .syntax_type_resolver
                .extract_type_from_declaration(parameter)
        })
}

/// Infer a lambda result using this candidate's SAM input types.
pub(super) fn infer_lambda_with_parameters(
    engine: &TypeInferenceEngine,
    lambda: &SyntaxNode,
    resolver: &SymbolResolver,
    parameter_types: &[Type],
) -> LambdaReturnEvidence {
    let parameters = lambda
        .children()
        .filter(|child| child.kind() == SyntaxKind::PARAM)
        .collect::<Vec<_>>();
    if parameters.len() != parameter_types.len() {
        return LambdaReturnEvidence::Incompatible;
    }
    let mut names = Vec::with_capacity(parameters.len());
    for (parameter, expected) in parameters.iter().zip(parameter_types) {
        let Some(name) = parameter
            .children_with_tokens()
            .filter_map(|child| child.into_token())
            .filter(|token| token.kind().is_identifier())
            .last()
            .map(|token| token.text().to_string())
        else {
            return LambdaReturnEvidence::Unknown;
        };
        if let Some(declared) = explicit_parameter_type(engine, parameter) {
            if declared == Type::Unknown {
                return LambdaReturnEvidence::Unknown;
            }
            if &declared != expected {
                return LambdaReturnEvidence::Incompatible;
            }
        }
        names.push(name);
    }
    engine.lambda_parameters.borrow_mut().push(
        names
            .into_iter()
            .zip(parameter_types.iter().cloned())
            .collect(),
    );
    let _scope = LambdaParameterScope(engine);
    let body = lambda.children().last();
    match (
        body.as_ref(),
        infer_lambda_body_return(engine, lambda, resolver),
    ) {
        (Some(body), Some(value)) if is_statement_expression(body) => {
            LambdaReturnEvidence::StatementExpression(value)
        }
        (_, Some(value)) => LambdaReturnEvidence::Value(value),
        (Some(body), None) if is_simple_void_block(body) => LambdaReturnEvidence::VoidOnly,
        _ => LambdaReturnEvidence::Unknown,
    }
}

fn is_statement_expression(body: &SyntaxNode) -> bool {
    match body.kind() {
        SyntaxKind::METHOD_CALL_EXPR | SyntaxKind::NEW_EXPR | SyntaxKind::ASSIGNMENT_EXPR => true,
        SyntaxKind::UNARY_EXPR => body.children_with_tokens().any(|child| {
            child.as_token().is_some_and(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::PLUS_PLUS | SyntaxKind::MINUS_MINUS
                )
            })
        }),
        _ => false,
    }
}

/// A throw or loop may prevent normal completion, making a value SAM legal
/// even without a return expression. Classify only plain blocks whose direct
/// statements have an evident void result; leave control flow uncertain.
fn is_simple_void_block(body: &SyntaxNode) -> bool {
    body.kind() == SyntaxKind::BLOCK_STMT
        && body.children().all(|statement| match statement.kind() {
            SyntaxKind::EXPR_STMT | SyntaxKind::LOCAL_VAR_DECL | SyntaxKind::EMPTY_STMT => true,
            SyntaxKind::RETURN_STMT => !statement
                .children()
                .any(|child| is_expression_kind(child.kind())),
            _ => false,
        })
}

/// Null branches add no lower bound; returns in nested functions are excluded.
fn infer_lambda_body_return(
    engine: &TypeInferenceEngine,
    lambda: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Option<Type> {
    if let Some(body) = lambda.children().last() {
        if is_expression_kind(body.kind()) {
            let inferred = infer_expression(engine, &body, resolver, None);
            return (!matches!(inferred, Type::Unknown | Type::Null)).then_some(inferred);
        }
    }
    let mut returns = Vec::new();
    for statement in lambda.descendants() {
        if statement.kind() != SyntaxKind::RETURN_STMT
            || statement
                .ancestors()
                .skip(1)
                .find(|ancestor| {
                    matches!(
                        ancestor.kind(),
                        SyntaxKind::LAMBDA_EXPR
                            | SyntaxKind::METHOD_DECL
                            | SyntaxKind::CLASS_DECL
                            | SyntaxKind::NEW_EXPR
                    )
                })
                .as_ref()
                != Some(lambda)
        {
            continue;
        }
        let Some(expression) = statement
            .children()
            .filter(|child| is_expression_kind(child.kind()))
            .last()
        else {
            continue;
        };
        let inferred = infer_expression(engine, &expression, resolver, None);
        match inferred {
            Type::Unknown => return None,
            Type::Null => {}
            concrete => returns.push(concrete),
        }
    }
    let mut returns = returns.into_iter();
    let mut result = returns.next()?;
    for next in returns {
        if result.is_assignable_from_with_workspace(&next, engine.workspace_index()) {
            continue;
        }
        result = if next.is_assignable_from_with_workspace(&result, engine.workspace_index()) {
            next
        } else {
            lub_two_types(engine, &result, &next)
        };
    }
    Some(result)
}
