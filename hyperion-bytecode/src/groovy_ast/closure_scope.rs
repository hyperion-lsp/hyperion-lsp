// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Deterministic free-variable analysis shared by parent and nested closures.

use hyperion_groovy::{Expression, GStringPart, Statement};
use std::collections::HashSet;

pub(super) fn free_variable_names(parameters: &[String], body: &[Statement]) -> Vec<String> {
    let mut defined: HashSet<String> = parameters.iter().cloned().collect();
    for statement in body {
        collect_defined(statement, &mut defined);
    }

    let mut referenced = Vec::new();
    let mut seen = HashSet::new();
    for statement in body {
        collect_referenced_statement(statement, &mut referenced, &mut seen);
    }
    referenced
        .into_iter()
        .filter(|name| !defined.contains(name))
        .collect()
}

fn collect_defined(statement: &Statement, defined: &mut HashSet<String>) {
    match statement {
        Statement::VariableDecl { name, .. } => {
            defined.insert(name.clone());
        }
        Statement::For { variable, body, .. } => {
            defined.insert(variable.clone());
            collect_defined_all(body, defined);
        }
        Statement::TraditionalFor { init, body, .. } => {
            if let Some(init) = init {
                collect_defined(init, defined);
            }
            collect_defined_all(body, defined);
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            collect_defined_all(then_body, defined);
            if let Some(else_body) = else_body {
                collect_defined_all(else_body, defined);
            }
        }
        Statement::While { body, .. } | Statement::Block(body) => {
            collect_defined_all(body, defined);
        }
        Statement::Switch {
            cases,
            default_case,
            ..
        } => {
            for case in cases {
                collect_defined_all(&case.body, defined);
            }
            if let Some(default_case) = default_case {
                collect_defined_all(default_case, defined);
            }
        }
        Statement::Try {
            body,
            catch_blocks,
            finally_block,
        } => {
            collect_defined_all(body, defined);
            for catch in catch_blocks {
                defined.insert(catch.variable_name.clone());
                collect_defined_all(&catch.body, defined);
            }
            if let Some(finally_block) = finally_block {
                collect_defined_all(finally_block, defined);
            }
        }
        Statement::Expression(_)
        | Statement::Throw(_)
        | Statement::Return(_)
        | Statement::Break
        | Statement::Continue
        | Statement::MethodDef { .. }
        | Statement::Unsupported(_) => {}
    }
}

fn collect_defined_all(statements: &[Statement], defined: &mut HashSet<String>) {
    for statement in statements {
        collect_defined(statement, defined);
    }
}

fn collect_referenced_statement(
    statement: &Statement,
    referenced: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    match statement {
        Statement::Expression(expression) | Statement::Throw(expression) => {
            collect_referenced_expression(expression, referenced, seen);
        }
        Statement::Return(Some(expression)) => {
            collect_referenced_expression(expression, referenced, seen);
        }
        Statement::VariableDecl {
            initializer: Some(expression),
            ..
        } => collect_referenced_expression(expression, referenced, seen),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            collect_referenced_expression(condition, referenced, seen);
            collect_referenced_all(then_body, referenced, seen);
            if let Some(else_body) = else_body {
                collect_referenced_all(else_body, referenced, seen);
            }
        }
        Statement::While { condition, body } => {
            collect_referenced_expression(condition, referenced, seen);
            collect_referenced_all(body, referenced, seen);
        }
        Statement::For { iterable, body, .. } => {
            collect_referenced_expression(iterable, referenced, seen);
            collect_referenced_all(body, referenced, seen);
        }
        Statement::TraditionalFor {
            init,
            condition,
            increment,
            body,
        } => {
            if let Some(init) = init {
                collect_referenced_statement(init, referenced, seen);
            }
            if let Some(condition) = condition {
                collect_referenced_expression(condition, referenced, seen);
            }
            if let Some(increment) = increment {
                collect_referenced_expression(increment, referenced, seen);
            }
            collect_referenced_all(body, referenced, seen);
        }
        Statement::Switch {
            expression,
            cases,
            default_case,
        } => {
            collect_referenced_expression(expression, referenced, seen);
            for case in cases {
                collect_referenced_expression(&case.pattern, referenced, seen);
                collect_referenced_all(&case.body, referenced, seen);
            }
            if let Some(default_case) = default_case {
                collect_referenced_all(default_case, referenced, seen);
            }
        }
        Statement::Try {
            body,
            catch_blocks,
            finally_block,
        } => {
            collect_referenced_all(body, referenced, seen);
            for catch in catch_blocks {
                collect_referenced_all(&catch.body, referenced, seen);
            }
            if let Some(finally_block) = finally_block {
                collect_referenced_all(finally_block, referenced, seen);
            }
        }
        Statement::Block(body) => collect_referenced_all(body, referenced, seen),
        Statement::VariableDecl {
            initializer: None, ..
        }
        | Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::MethodDef { .. }
        | Statement::Unsupported(_) => {}
    }
}

fn collect_referenced_all(
    statements: &[Statement],
    referenced: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    for statement in statements {
        collect_referenced_statement(statement, referenced, seen);
    }
}

fn collect_referenced_expression(
    expression: &Expression,
    referenced: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    match expression {
        Expression::Identifier(name) | Expression::Variable(name) => {
            if seen.insert(name.clone()) {
                referenced.push(name.clone());
            }
        }
        Expression::BinaryOp { left, right, .. } => {
            collect_referenced_expression(left, referenced, seen);
            collect_referenced_expression(right, referenced, seen);
        }
        Expression::UnaryOp { operand, .. } | Expression::Parenthesized(operand) => {
            collect_referenced_expression(operand, referenced, seen);
        }
        Expression::MethodCall {
            receiver,
            arguments,
            ..
        } => {
            if let Some(receiver) = receiver {
                collect_referenced_expression(receiver, referenced, seen);
            }
            collect_referenced_expressions(arguments, referenced, seen);
        }
        Expression::PropertyAccess { object, .. } => {
            collect_referenced_expression(object, referenced, seen);
        }
        Expression::IndexAccess { object, index } => {
            collect_referenced_expression(object, referenced, seen);
            collect_referenced_expression(index, referenced, seen);
        }
        Expression::Closure(closure) => {
            let mut parameters = closure.parameters.clone();
            if closure.implicit_it && parameters.is_empty() {
                parameters.push("it".to_string());
            }
            for name in free_variable_names(&parameters, &closure.body) {
                if seen.insert(name.clone()) {
                    referenced.push(name);
                }
            }
        }
        Expression::ListLiteral(elements) => {
            collect_referenced_expressions(elements, referenced, seen);
        }
        Expression::MapLiteral(entries) => {
            for (_, value) in entries {
                collect_referenced_expression(value, referenced, seen);
            }
        }
        Expression::RangeLiteral { start, end, .. } => {
            collect_referenced_expression(start, referenced, seen);
            collect_referenced_expression(end, referenced, seen);
        }
        Expression::GString(parts) => {
            for part in parts {
                if let GStringPart::Interpolation(value) = part {
                    collect_referenced_expression(value, referenced, seen);
                }
            }
        }
        Expression::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_referenced_expression(condition, referenced, seen);
            collect_referenced_expression(then_expr, referenced, seen);
            collect_referenced_expression(else_expr, referenced, seen);
        }
        Expression::ElvisOp { primary, fallback } => {
            collect_referenced_expression(primary, referenced, seen);
            collect_referenced_expression(fallback, referenced, seen);
        }
        Expression::NewObject { arguments, .. } => {
            collect_referenced_expressions(arguments, referenced, seen);
        }
        Expression::IntLiteral(_)
        | Expression::LongLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::DoubleLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::NullLiteral
        | Expression::Unsupported(_) => {}
    }
}

fn collect_referenced_expressions(
    expressions: &[Expression],
    referenced: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    for expression in expressions {
        collect_referenced_expression(expression, referenced, seen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_groovy::ClosureExpr;

    #[test]
    fn free_variables_are_source_ordered_and_include_nested_requirements() {
        let body = vec![Statement::Expression(Expression::ListLiteral(vec![
            Expression::Identifier("second".to_string()),
            Expression::Closure(Box::new(ClosureExpr {
                parameters: vec!["nested_parameter".to_string()],
                body: vec![Statement::Expression(Expression::ListLiteral(vec![
                    Expression::Identifier("first".to_string()),
                    Expression::Identifier("nested_parameter".to_string()),
                    Expression::Identifier("second".to_string()),
                ]))],
                implicit_it: false,
            })),
        ]))];

        assert_eq!(
            free_variable_names(&[], &body),
            ["second".to_string(), "first".to_string()]
        );
    }
}
