// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Expression type inference
//!
//! This module contains all expression-level type inference logic including:
//! - Literals (int, string, boolean, etc.)
//! - Identifiers (variables, fields, parameters)
//! - Method calls and field access
//! - Array access
//! - Binary and unary expressions
//! - Cast and new expressions
//! - This and super
//! - Switch expressions
//! - Functional expressions (lambdas, method refs, constructor refs)
//!
//! # Migration Status
//!
//! This module is being incrementally migrated to a visitor pattern using
//! type-safe expression wrappers in `expr_wrappers/`. The following have
//! been migrated:
//! - Literals (INT_LITERAL, STRING_LITERAL, etc.) → expr_wrappers::literals
//! - Identifiers (IDENT) → expr_wrappers::identifiers
//! - Simple expressions (this, super, paren, cast, new) → expr_wrappers::simple
//!
//! Migration continues incrementally as wrappers are adopted in this module.

use super::expr_wrappers::{
    identifiers::IdentifierExpr,
    literals::LiteralExpr,
    simple::{CastExpr, NewExpr, ParenExpr, SuperExpr, ThisExpr},
};
use super::lambda::infer_lambda_with_parameters;
use super::visitor::InferType;
use super::TypeInferenceEngine;
use crate::call_resolution::{resolve_call_with_lambda_inference, LambdaReturnEvidence};
use crate::resolver::SymbolResolver;
use crate::symbol::{ClassType, PrimitiveType, Type};
use crate::types::expr_types;
use hyperion_parser::syntax::{SyntaxKind, SyntaxNode};
use hyperion_workspace::{Language, TypeNameResolution};
use lsp_types::SymbolKind as LspSymbolKind;
use rowan::NodeOrToken;
use tracing::debug;

/// Main expression inference dispatcher
///
/// This is called from TypeInferenceEngine::infer_expression and dispatches
/// to specialized methods based on syntax kind.
pub(super) fn infer_expression(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
    expected_type: Option<&Type>,
) -> Type {
    if expected_type.is_none() && engine.lambda_parameters.borrow().is_empty() {
        if let Some(cached) = engine.cached_expression_type(node.text_range()) {
            return cached;
        }
    }

    let inferred = infer_expression_uncached(engine, node, resolver, expected_type);
    if expected_type.is_none() && engine.lambda_parameters.borrow().is_empty() {
        engine.cache_expression_type(node.text_range(), inferred.clone());
    }
    inferred
}

fn infer_expression_uncached(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
    expected_type: Option<&Type>,
) -> Type {
    let kind = node.kind();
    debug!("Inferring type for node kind: {:?}", kind);

    match kind {
        // Wrapper literal expression node (parser shape)
        SyntaxKind::LITERAL_EXPR => infer_literal_expr(engine, node, resolver, expected_type),

        // Literals - migrated to visitor pattern
        SyntaxKind::INT_LITERAL
        | SyntaxKind::LONG_LITERAL
        | SyntaxKind::FLOAT_LITERAL
        | SyntaxKind::DOUBLE_LITERAL
        | SyntaxKind::CHAR_LITERAL
        | SyntaxKind::BOOLEAN_LITERAL
        | SyntaxKind::STRING_LITERAL
        | SyntaxKind::TEXT_BLOCK_LITERAL
        | SyntaxKind::NULL_LITERAL => LiteralExpr::cast(node).map_or(Type::Unknown, |expr| {
            expr.infer_type(engine, resolver, expected_type)
        }),

        // Identifiers - migrated to visitor pattern
        SyntaxKind::IDENT => IdentifierExpr::cast(node).map_or(Type::Unknown, |expr| {
            expr.infer_type(engine, resolver, expected_type)
        }),

        // Name expression wrapper (e.g., NAME_EXPR -> IDENT)
        SyntaxKind::NAME_EXPR => infer_name_expr(engine, node, resolver),

        // Method calls
        SyntaxKind::METHOD_CALL_EXPR => infer_method_call(engine, node, resolver),

        // Field access
        SyntaxKind::FIELD_ACCESS_EXPR => infer_field_access(engine, node, resolver),

        // Array access
        SyntaxKind::ARRAY_ACCESS_EXPR => infer_array_access(engine, node, resolver),

        // Binary expressions
        SyntaxKind::BINARY_EXPR => infer_binary_expr(engine, node, resolver),

        // Unary expressions
        SyntaxKind::UNARY_EXPR => infer_unary_expr(engine, node, resolver),

        // Ternary (conditional) expression
        SyntaxKind::TERNARY_EXPR => infer_ternary_expr(engine, node, resolver),

        // Simple expressions - migrated to visitor pattern
        SyntaxKind::PAREN_EXPR => ParenExpr::cast(node).map_or(Type::Unknown, |expr| {
            expr.infer_type(engine, resolver, expected_type)
        }),

        SyntaxKind::CAST_EXPR => CastExpr::cast(node).map_or(Type::Unknown, |expr| {
            expr.infer_type(engine, resolver, expected_type)
        }),

        SyntaxKind::NEW_EXPR => NewExpr::cast(node).map_or(Type::Unknown, |expr| {
            expr.infer_type(engine, resolver, expected_type)
        }),

        SyntaxKind::THIS_EXPR => ThisExpr::cast(node).map_or(Type::Unknown, |expr| {
            expr.infer_type(engine, resolver, expected_type)
        }),

        SyntaxKind::SUPER_EXPR => SuperExpr::cast(node).map_or(Type::Unknown, |expr| {
            expr.infer_type(engine, resolver, expected_type)
        }),

        // Lambda expressions - infer from expected type
        SyntaxKind::LAMBDA_EXPR => expected_type.map_or_else(
            || {
                // No expected type - fallback to Unknown
                debug!("Lambda without expected type context");
                Type::Unknown
            },
            |expected| infer_lambda_with_expected_type(engine, expected),
        ),

        // Method reference (e.g., User::getName, String::toUpperCase)
        SyntaxKind::METHOD_REF_EXPR => infer_method_reference(engine, node, resolver),

        // Constructor reference (e.g., ArrayList::new, String[]::new)
        SyntaxKind::CONSTRUCTOR_REF_EXPR => {
            infer_constructor_reference(engine, node, resolver, expected_type)
        }

        // Switch expression (Java 12+)
        SyntaxKind::SWITCH_EXPR => infer_switch_expression(engine, node, resolver),

        _ => {
            debug!("Unknown expression kind for type inference: {:?}", kind);
            Type::Unknown
        }
    }
}

fn infer_literal_expr(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
    expected_type: Option<&Type>,
) -> Type {
    // Preferred: recurse into literal child nodes.
    for child in node.children() {
        let inferred = infer_expression(engine, &child, resolver, expected_type);
        if !matches!(inferred, Type::Unknown) {
            return inferred;
        }
    }

    // Fallback: handle token-only wrapper shapes.
    for child in node.children_with_tokens() {
        if let NodeOrToken::Token(token) = child {
            let ty = match token.kind() {
                SyntaxKind::INT_LITERAL => Type::Primitive(PrimitiveType::Int),
                SyntaxKind::LONG_LITERAL => Type::Primitive(PrimitiveType::Long),
                SyntaxKind::FLOAT_LITERAL => Type::Primitive(PrimitiveType::Float),
                SyntaxKind::DOUBLE_LITERAL => Type::Primitive(PrimitiveType::Double),
                SyntaxKind::CHAR_LITERAL => Type::Primitive(PrimitiveType::Char),
                SyntaxKind::BOOLEAN_LITERAL => Type::Primitive(PrimitiveType::Boolean),
                SyntaxKind::STRING_LITERAL | SyntaxKind::TEXT_BLOCK_LITERAL => {
                    Type::Class(ClassType::string())
                }
                SyntaxKind::NULL_LITERAL => Type::Null,
                _ => continue,
            };
            return ty;
        }
    }

    Type::Unknown
}

fn infer_name_expr(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    let name = node.text().to_string().trim().to_string();
    if name.is_empty() {
        return Type::Unknown;
    }
    if let Some(ty) = engine
        .lambda_parameters
        .borrow()
        .iter()
        .rev()
        .find_map(|parameters| parameters.get(&name).cloned())
    {
        return ty;
    }

    let offset = u32::from(node.text_range().start());
    let Some(scope_id) = resolver.find_scope_at_offset(offset) else {
        return Type::Unknown;
    };
    if let Some(symbol) = resolver.resolve_in_scope_at_offset(&name, scope_id, offset) {
        if !matches!(symbol.ty, Type::Unknown) {
            return symbol.ty;
        }
    }

    Type::Unknown
}

// Note: infer_literal() and infer_identifier() have been migrated to
// expr_wrappers::literals and expr_wrappers::identifiers respectively.
// Additional expression kinds will migrate as wrapper coverage expands.

/// Infer the type of a method call expression
///
/// For a method call like `obj.method(args)`, this:
/// 1. Infers the type of the receiver (`obj`)
/// 2. Looks up the method in that type via WorkspaceIndex
/// 3. Returns the method's return type
fn infer_method_call(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    debug!("Inferring method call type");

    // Find the receiver (the expression before the dot) and the method name
    let mut receiver_type = find_receiver_type(engine, node, resolver);
    let receiver_was_unknown = matches!(receiver_type, Type::Unknown);
    let method_name = find_method_name(node);
    if matches!(receiver_type, Type::Unknown)
        && !node.children_with_tokens().any(|element| {
            element
                .as_token()
                .is_some_and(|token| token.kind() == SyntaxKind::DOT)
        })
    {
        receiver_type = resolver
            .get_enclosing_class_at_offset(u32::from(node.text_range().start()))
            .map_or(Type::Unknown, Type::Class);
    }
    let arguments = argument_expressions(node);
    let arg_types = arguments
        .iter()
        .map(|argument| infer_expression(engine, argument, resolver, None))
        .collect::<Vec<_>>();
    let lambda_positions = arguments
        .iter()
        .enumerate()
        .filter(|(_, argument)| argument.kind() == SyntaxKind::LAMBDA_EXPR)
        .map(|(index, argument)| engine.lambda_argument(index, argument))
        .collect::<Vec<_>>();

    if let Some(method_name) = method_name.as_ref() {
        debug!("Method call: {} on type {:?}", method_name, receiver_type);

        if let Type::Class(class_type) = &receiver_type {
            let class_fqn = class_type.qualified_name();
            if let Some(resolved) = resolve_call_with_lambda_inference(
                engine.workspace_index(),
                &class_fqn,
                Some(&receiver_type),
                method_name,
                &arg_types,
                &lambda_positions,
                |index, parameter_types| {
                    arguments
                        .get(index)
                        .map_or(LambdaReturnEvidence::Unknown, |lambda| {
                            infer_lambda_with_parameters(engine, lambda, resolver, parameter_types)
                        })
                },
            ) {
                debug!(
                    "Selected overload {} with confidence {:?}",
                    resolved.declaration.fqn, resolved.confidence
                );
                return resolved.return_type;
            }
        }
    }

    // Mockito-style static helper: verify(mock).foo()
    // When receiver inference fails but the first argument type is known,
    // propagate that type as the call result so chained navigation works.
    if receiver_was_unknown && method_name.as_deref() == Some("verify") {
        if let Some(first_arg) = arg_types.first() {
            if !matches!(first_arg, Type::Unknown) {
                debug!(
                    "Heuristic: treating verify(...) return type as first argument type: {:?}",
                    first_arg
                );
                return first_arg.clone();
            }
        }
    }

    debug!(
        "Type::Unknown: method call return type unresolved (node text: '{}')",
        node.text().to_string().chars().take(60).collect::<String>()
    );
    Type::Unknown
}

/// Infer the type of a field access expression
///
/// For a field access like `obj.field`, this:
/// 1. Infers the type of the receiver (`obj`)
/// 2. Looks up the field in that type via WorkspaceIndex
/// 3. Returns the field's type
fn infer_field_access(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    debug!("Inferring field access type");

    // Find the receiver and field name
    let receiver_type = find_receiver_type(engine, node, resolver);
    let field_name = find_field_name(node);

    if let Some(field_name) = field_name {
        debug!("Field access: {} on type {:?}", field_name, receiver_type);

        if let Some(field_type) =
            engine.resolve_field_type_cached(resolver, &receiver_type, &field_name)
        {
            return field_type;
        }
    }

    debug!(
        "Type::Unknown: field access unresolved (receiver={:?})",
        receiver_type
    );
    Type::Unknown
}

/// Infer the type of an array access expression
///
/// For `array[index]`, returns the element type of the array.
fn infer_array_access(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    debug!("Inferring array access type");

    // Find the array expression (first child)
    if let Some(array_expr) = node.children().next() {
        let array_type = infer_expression(engine, &array_expr, resolver, None);

        // If it's an array type, return the element type
        if let Type::Array {
            element_type,
            dimensions,
        } = array_type
        {
            if dimensions == 1 {
                return *element_type;
            }
            // Multi-dimensional array: reduce dimensions by 1
            return Type::Array {
                element_type,
                dimensions: dimensions - 1,
            };
        }
    }

    Type::Unknown
}

/// Infer the type of a binary expression
///
/// Handles arithmetic, logical, and comparison operators.
fn infer_binary_expr(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    debug!("Inferring binary expression type");

    // Find the operator: the first non-trivia token child. The parser bumps
    // trivia (whitespace/comments) inside BINARY_EXPR, so the first token
    // child of `a + b` is usually WHITESPACE, not the operator.
    let operator = node
        .children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| !token.kind().is_trivia());

    // Find the operands. Same-precedence chains parse as continuation nodes:
    // `a + b + c` produces sibling BINARY_EXPR nodes where the second is
    // `[PLUS, NAME_EXPR(c)]` — its left operand is the preceding sibling
    // expression (pinned by the parser's expression_tree_tests).
    let mut operands = node.children();
    let first = operands.next();
    let second = operands.next();
    let (left, right) = match (first, second) {
        (Some(left), Some(right)) => (Some(left), Some(right)),
        (Some(right), None) => (
            node.prev_sibling()
                .filter(|sibling| is_expression_kind(sibling.kind())),
            Some(right),
        ),
        _ => (None, None),
    };

    if let (Some(left), Some(operator), Some(right)) = (left, operator, right) {
        let left_type = infer_expression(engine, &left, resolver, None);
        let right_type = infer_expression(engine, &right, resolver, None);

        // Determine result type based on operator
        return match operator.kind() {
            // Arithmetic operators
            SyntaxKind::PLUS
            | SyntaxKind::MINUS
            | SyntaxKind::STAR
            | SyntaxKind::SLASH
            | SyntaxKind::PERCENT => expr_types::binary_arithmetic_type(&left_type, &right_type),

            // Comparison operators
            SyntaxKind::EQ_EQ
            | SyntaxKind::BANG_EQ
            | SyntaxKind::LT
            | SyntaxKind::LT_EQ
            | SyntaxKind::GT
            | SyntaxKind::GT_EQ => expr_types::comparison_type(&left_type, &right_type),

            // Logical operators
            SyntaxKind::AMP_AMP | SyntaxKind::PIPE_PIPE => {
                expr_types::logical_type(&left_type, &right_type)
            }

            // Bitwise operators
            SyntaxKind::AMP | SyntaxKind::PIPE | SyntaxKind::CARET => {
                expr_types::bitwise_type(&left_type, &right_type)
            }

            _ => Type::Unknown,
        };
    }

    Type::Unknown
}

/// Infer the type of a unary expression
fn infer_unary_expr(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    // For most unary expressions, the type is the same as the operand
    node.children().next().map_or(Type::Unknown, |operand| {
        infer_expression(engine, &operand, resolver, None)
    })
}

/// Infer the type of a ternary (conditional) expression
fn infer_ternary_expr(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    // For `condition ? trueExpr : falseExpr`, find the LUB of both branches
    let mut children = node.children();
    children.next(); // Skip condition

    let true_type = if let Some(true_expr) = children.next() {
        infer_expression(engine, &true_expr, resolver, None)
    } else {
        return Type::Unknown;
    };

    let false_type = if let Some(false_expr) = children.next() {
        infer_expression(engine, &false_expr, resolver, None)
    } else {
        return true_type; // Only true branch exists, use its type
    };

    // Find the least upper bound of both branch types
    lub_two_types(engine, &true_type, &false_type)
}

// Note: infer_cast_expr(), infer_new_expr(), infer_this(), and infer_super()
// have been migrated to expr_wrappers::simple.

/// Infer the type of a switch expression (Java 12+)
///
/// Switch expressions return a value, and the type is the common supertype
/// of all arm expressions (after -> or from yield statements).
fn infer_switch_expression(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    debug!("Inferring switch expression type");

    let mut arm_types: Vec<Type> = Vec::new();

    // Find all CASE_CLAUSE children
    for child in node.children() {
        if child.kind() == SyntaxKind::CASE_CLAUSE {
            if let Some(arm_type) = infer_case_clause_type(engine, &child, resolver) {
                debug!("Case clause type: {:?}", arm_type);
                arm_types.push(arm_type);
            }
        }
    }

    if arm_types.is_empty() {
        debug!("No arm types found in switch expression");
        return Type::Unknown;
    }

    // Find common supertype of all arm types
    find_common_supertype(engine, &arm_types)
}

/// Infer the type of a case clause's result expression
///
/// Handles both arrow syntax (`case X -> expr`) and block syntax with yield.
fn infer_case_clause_type(
    engine: &TypeInferenceEngine,
    case_clause: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Option<Type> {
    // Look for arrow (->) followed by expression, or block containing yield
    let mut found_arrow = false;

    for child in case_clause.children_with_tokens() {
        match child {
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::ARROW => {
                found_arrow = true;
            }
            NodeOrToken::Node(node) if found_arrow => {
                // Expression or block after arrow
                if node.kind() == SyntaxKind::BLOCK_STMT {
                    // Block syntax - look for yield statements
                    return find_yield_type(engine, &node, resolver);
                } else if node.kind() == SyntaxKind::THROW_STMT {
                    // throw statements don't contribute to the result type
                    return None;
                }
                // Direct expression after arrow
                return Some(infer_expression(engine, &node, resolver, None));
            }
            NodeOrToken::Token(token) if token.kind() == SyntaxKind::COLON => {
                // Traditional colon syntax - look for yield in statements
            }
            NodeOrToken::Node(node) if node.kind() == SyntaxKind::YIELD_STMT => {
                // Found a yield statement in traditional syntax
                return extract_yield_expression_type(engine, &node, resolver);
            }
            _ => {}
        }
    }

    // For traditional colon syntax, check descendants for yield
    for descendant in case_clause.descendants() {
        if descendant.kind() == SyntaxKind::YIELD_STMT {
            return extract_yield_expression_type(engine, &descendant, resolver);
        }
    }

    None
}

/// Find the type of yield statements in a block
fn find_yield_type(
    engine: &TypeInferenceEngine,
    block: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Option<Type> {
    // Search for YIELD_STMT in the block
    for descendant in block.descendants() {
        if descendant.kind() == SyntaxKind::YIELD_STMT {
            return extract_yield_expression_type(engine, &descendant, resolver);
        }
    }
    None
}

/// Extract the type of the expression in a yield statement
fn extract_yield_expression_type(
    engine: &TypeInferenceEngine,
    yield_stmt: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Option<Type> {
    // YIELD_STMT structure: yield expression ;
    for child in yield_stmt.children() {
        // Skip YIELD_KW token, find the expression
        if is_expression_kind(child.kind()) {
            return Some(infer_expression(engine, &child, resolver, None));
        }
    }
    None
}

/// Find the common supertype of multiple types
///
/// This implements a simplified version of the "least upper bound" (LUB)
/// algorithm from the JLS for ternary and switch expressions.
pub(super) fn find_common_supertype(engine: &TypeInferenceEngine, types: &[Type]) -> Type {
    if types.is_empty() {
        return Type::Unknown;
    }

    if types.len() == 1 {
        return types[0].clone();
    }

    // Start with the first type
    let mut result = types[0].clone();

    for ty in types.iter().skip(1) {
        result = lub_two_types(engine, &result, ty);
        if result == Type::Unknown {
            // If we hit Unknown, keep going - we might recover with Object
            result = Type::Class(ClassType::object());
        }
    }

    result
}

/// Compute the least upper bound (LUB) of two types
#[allow(clippy::only_used_in_recursion)]
pub(super) fn lub_two_types(engine: &TypeInferenceEngine, t1: &Type, t2: &Type) -> Type {
    // Same type - return it
    if t1 == t2 {
        return t1.clone();
    }

    // A branch of unknown type makes the whole conditional unknown. Returning
    // the other branch's type would present a guess as fact: with
    // `cond ? <unknown double> : 0` the result is a double, and claiming `int`
    // made every `Double x = ...` initializer a false type error.
    if matches!(t1, Type::Unknown) || matches!(t2, Type::Unknown) {
        return Type::Unknown;
    }

    // Handle Null - null is assignable to any reference type
    if matches!(t1, Type::Null) {
        return t2.clone();
    }
    if matches!(t2, Type::Null) {
        return t1.clone();
    }

    // Handle primitives - use numeric promotion
    if let (Type::Primitive(p1), Type::Primitive(p2)) = (t1, t2) {
        return numeric_lub(*p1, *p2);
    }

    // Mixed primitive and reference - box the primitive and try again
    if let Type::Primitive(p) = t1 {
        let boxed = Type::Class(ClassType::new(
            p.wrapper_class().to_string(),
            Some("java.lang".into()),
        ));
        return lub_two_types(engine, &boxed, t2);
    }
    if let Type::Primitive(p) = t2 {
        let boxed = Type::Class(ClassType::new(
            p.wrapper_class().to_string(),
            Some("java.lang".into()),
        ));
        return lub_two_types(engine, t1, &boxed);
    }

    // Both reference types - check for assignability
    if let (Type::Class(c1), Type::Class(c2)) = (t1, t2) {
        // If one is assignable from the other, return the more general one
        if t1.is_assignable_from(t2) {
            return t1.clone();
        }
        if t2.is_assignable_from(t1) {
            return t2.clone();
        }

        // Check for common numeric wrapper hierarchy
        let numeric_wrappers = ["Integer", "Long", "Float", "Double", "Short", "Byte"];
        if numeric_wrappers.iter().any(|&w| w == c1.name)
            && numeric_wrappers.iter().any(|&w| w == c2.name)
        {
            return Type::Class(ClassType::new("Number".into(), Some("java.lang".into())));
        }

        // Fall back to Object for unrelated reference types
        return Type::Class(ClassType::object());
    }

    // Arrays - check element types
    if let (
        Type::Array {
            element_type: e1,
            dimensions: d1,
        },
        Type::Array {
            element_type: e2,
            dimensions: d2,
        },
    ) = (t1, t2)
    {
        if d1 == d2 {
            // Same dimensions - LUB of element types
            let elem_lub = lub_two_types(engine, e1, e2);
            if !matches!(elem_lub, Type::Unknown) {
                return Type::Array {
                    element_type: Box::new(elem_lub),
                    dimensions: *d1,
                };
            }
        }
        // Different dimensions or incompatible elements - return Object
        return Type::Class(ClassType::object());
    }

    // Default fallback
    Type::Class(ClassType::object())
}

/// Compute the LUB of two primitive types
fn numeric_lub(p1: PrimitiveType, p2: PrimitiveType) -> Type {
    use PrimitiveType::{Boolean, Double, Float, Int, Long};

    // Boolean is not compatible with numeric types
    if p1 == Boolean || p2 == Boolean {
        if p1 == p2 {
            return Type::Primitive(Boolean);
        }
        // L12: Mixed boolean and numeric types require boxing to Object
        tracing::warn!(
            "L12: Mixed primitive types ({:?}, {:?}) require boxing to Object. \
             This may indicate a logic error in switch expression or conditional.",
            p1,
            p2
        );
        return Type::Class(ClassType::object());
    }

    // Numeric promotion rules (JLS §5.6)
    Type::Primitive(match (p1, p2) {
        // Double wins
        (Double, _) | (_, Double) => Double,
        // Float wins against non-double
        (Float, _) | (_, Float) => Float,
        // Long wins against smaller types
        (Long, _) | (_, Long) => Long,
        // Everything else promotes to int
        _ => Int,
    })
}

/// Find the receiver type for a method call or field access
fn find_receiver_type(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    // The receiver is typically the first child that's an expression
    node.children().next().map_or(Type::Unknown, |receiver| {
        if receiver.kind() == SyntaxKind::NAME_EXPR
            && node.children_with_tokens().any(|child| {
                child
                    .as_token()
                    .is_some_and(|token| token.kind() == SyntaxKind::DOT)
            })
        {
            return infer_receiver_name(engine, &receiver, resolver);
        }
        infer_expression(engine, &receiver, resolver, None)
    })
}

/// A name before `.` may denote a value or a type. Resolve the value namespace
/// first; only an unbound name may become a static type receiver.
fn infer_receiver_name(
    engine: &TypeInferenceEngine,
    receiver: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    let name = receiver.text().to_string();
    let name = name.trim();
    if let Some(ty) = engine
        .lambda_parameters
        .borrow()
        .iter()
        .rev()
        .find_map(|parameters| parameters.get(name).cloned())
    {
        return ty;
    }
    let offset = u32::from(receiver.text_range().start());
    if let Some(scope) = resolver.find_scope_at_offset(offset) {
        if let Some(symbol) = resolver.resolve_in_scope_at_offset(name, scope, offset) {
            return symbol.ty;
        }
    }

    let Some(root) = receiver.ancestors().last() else {
        return Type::Unknown;
    };
    let imports = engine.imports_for(&root);
    let import_resolver =
        hyperion_workspace::import_resolver::ImportResolver::new(imports, engine.workspace_index());

    let mut owners = Vec::new();
    let mut class_names = receiver
        .ancestors()
        .filter(|ancestor| {
            matches!(
                ancestor.kind(),
                SyntaxKind::CLASS_DECL
                    | SyntaxKind::INTERFACE_DECL
                    | SyntaxKind::ENUM_DECL
                    | SyntaxKind::RECORD_DECL
            )
        })
        .filter_map(|declaration| {
            declaration.children_with_tokens().find_map(|child| {
                child
                    .into_token()
                    .filter(|token| token.kind() == SyntaxKind::IDENT)
                    .map(|token| token.text().to_string())
            })
        })
        .collect::<Vec<_>>();
    class_names.reverse();
    let mut owner = imports.current_package.clone().unwrap_or_default();
    for class_name in class_names {
        if !owner.is_empty() {
            owner.push('.');
        }
        owner.push_str(&class_name);
        owners.push(owner.clone());
    }
    owners.reverse();

    for owner in &owners {
        if import_resolver.has_value_member_named(owner, name) {
            let owner_type = Type::Class(class_type_from_fqn(owner));
            return resolver
                .resolve_field_type(&owner_type, name)
                .unwrap_or(Type::Unknown);
        }
    }
    match import_resolver.resolve_static_values(name).as_slice() {
        [] => {}
        [value] => {
            let Some(declaration) = engine.workspace_index().find_symbol_definition(
                &hyperion_workspace::SymbolTarget::new(
                    value.fqn.as_ref(),
                    hyperion_workspace::DeclarationNamespace::Value,
                ),
            ) else {
                return Type::Unknown;
            };
            let Some(owner) = declaration.symbol.container.as_deref() else {
                return Type::Unknown;
            };
            let Some(written_type) = declaration.symbol.return_type.as_deref() else {
                return Type::Unknown;
            };
            return crate::resolver::type_parameter_mapping::parse_type_in_declaration_exact(
                engine.workspace_index(),
                owner,
                written_type,
                &std::collections::HashSet::new(),
            )
            .unwrap_or(Type::Unknown);
        }
        _ => return Type::Unknown,
    }

    // Type parameters bind names in a separate namespace, but prevent an
    // identically named imported class from being elected as this receiver.
    if hyperion_types::enclosing_type_parameter_names(receiver)
        .iter()
        .any(|parameter| parameter == name)
    {
        return Type::Unknown;
    }

    match engine.workspace_index().resolve_type_name_in_context(
        imports,
        &owners,
        name,
        Language::Java,
    ) {
        TypeNameResolution::Exact(fqn) => Type::Class(class_type_from_fqn(&fqn)),
        TypeNameResolution::Missing | TypeNameResolution::Ambiguous(_) => Type::Unknown,
    }
}

fn class_type_from_fqn(fqn: &str) -> ClassType {
    let (package, name) = fqn.rsplit_once('.').map_or((None, fqn), |(package, name)| {
        (Some(package.to_string()), name)
    });
    ClassType::new(name.to_string(), package)
}

/// Find the method name from a method call expression
fn find_method_name(node: &SyntaxNode) -> Option<String> {
    // Look for an IDENT token that represents the method name
    if let Some(name) = node.children_with_tokens().find_map(|child| {
        child
            .as_token()
            .filter(|token| token.kind() == SyntaxKind::IDENT)
            .map(|token| token.text().to_string())
    }) {
        return Some(name);
    }

    // Fallback: unqualified calls like `verify(...)` have the IDENT inside NAME_EXPR
    node.descendants_with_tokens().find_map(|child| {
        child
            .as_token()
            .filter(|token| token.kind() == SyntaxKind::IDENT)
            .map(|token| token.text().to_string())
    })
}

/// Find the field name from a field access expression
fn find_field_name(node: &SyntaxNode) -> Option<String> {
    // Look for an IDENT token that represents the field name
    node.children_with_tokens().find_map(|child| {
        if let NodeOrToken::Token(token) = child {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
        None
    })
}

/// Infer types of method call arguments
fn argument_expressions(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut arguments = Vec::new();

    // Find ARG_LIST child
    for child in node.children() {
        if child.kind() == SyntaxKind::ARG_LIST {
            // Use comma-based argument extraction
            let mut current_arg: Option<SyntaxNode> = None;

            for arg_child in child.children_with_tokens() {
                match arg_child {
                    // Record only the FIRST expression node for this argument.
                    NodeOrToken::Node(expr_node)
                        if current_arg.is_none() && is_expression_kind(expr_node.kind()) =>
                    {
                        current_arg = Some(expr_node.clone());
                    }
                    NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => {
                        // Comma marks end of current argument
                        if let Some(arg) = current_arg.take() {
                            arguments.push(arg);
                        }
                        current_arg = None;
                    }
                    _ => {}
                }
            }

            // Don't forget the last argument
            if let Some(arg) = current_arg.take() {
                arguments.push(arg);
            }

            break;
        }
    }

    arguments
}

/// Check if a SyntaxKind represents an expression node
pub(super) const fn is_expression_kind(kind: SyntaxKind) -> bool {
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
            | SyntaxKind::THIS_EXPR
            | SyntaxKind::SUPER_EXPR
            | SyntaxKind::LAMBDA_EXPR
            | SyntaxKind::ARRAY_ACCESS_EXPR
            | SyntaxKind::ARRAY_INIT_EXPR
    )
}

/// Infer lambda expression type from expected functional interface type
///
/// When a lambda is used in a context expecting a functional interface,
/// we can infer the lambda's type from the Single Abstract Method (SAM).
pub(super) fn infer_lambda_with_expected_type(
    engine: &TypeInferenceEngine,
    expected_type: &Type,
) -> Type {
    debug!(
        "Inferring lambda type from expected type: {:?}",
        expected_type
    );

    // Extract FQN from expected type
    let Type::Class(ct) = expected_type else {
        debug!("Expected type is not a class, cannot infer lambda");
        return Type::Unknown;
    };
    let fqn = ct
        .package
        .as_ref()
        .map_or_else(|| ct.name.clone(), |pkg| format!("{}.{}", pkg, ct.name));

    debug!("Looking up functional interface: {}", fqn);

    let type_arguments = ct
        .type_arguments
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let hyperion_workspace::FunctionalInterfaceQuery::Resolved(signature) = engine
        .workspace_index()
        .query_functional_interface(&fqn, &type_arguments)
    else {
        debug!("{} does not have a Java function type", fqn);
        return Type::Unknown;
    };

    debug!(
        "Found SAM: {}.{} returns {}",
        fqn, signature.method.simple_name, signature.return_type
    );

    let Some(parsed) = engine.parse_type_string(&signature.return_type) else {
        debug!(
            "Type::Unknown: failed to parse SAM return type '{}' for {}.{}",
            signature.return_type, fqn, signature.method.simple_name
        );
        return Type::Unknown;
    };
    parsed
}

/// Infer the type of a method reference expression
fn infer_method_reference(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
) -> Type {
    debug!("Inferring method reference type");

    // Extract receiver and method name
    let mut receiver_text: Option<String> = None;
    let mut method_name: Option<String> = None;

    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) => {
                if receiver_text.is_none() {
                    receiver_text = Some(n.text().to_string().trim().to_string());
                }
            }
            NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::IDENT {
                    method_name = Some(t.text().to_string());
                }
            }
        }
    }

    let (receiver_text, method_name) = if let (Some(r), Some(m)) = (receiver_text, method_name) {
        (r, m)
    } else {
        debug!("Failed to extract method reference components");
        return Type::Unknown;
    };

    debug!("Method reference: {}::{}", receiver_text, method_name);

    // Resolve the receiver type
    let receiver_type = if receiver_text == "this" {
        if let Some(class_type) = resolver.get_enclosing_class() {
            Type::Class(class_type)
        } else {
            debug!("Could not determine enclosing class for 'this'");
            return Type::Unknown;
        }
    } else if let Some(symbol) = resolver.resolve_identifier(&receiver_text, node) {
        symbol.type_info
    } else if let Some(t) = engine.parse_type_string(&receiver_text) {
        t
    } else {
        debug!("Failed to parse receiver type: {}", receiver_text);
        return Type::Unknown;
    };

    debug!("Resolved receiver type: {:?}", receiver_type);

    // Look up the method on the receiver type
    if let Type::Class(class_type) = &receiver_type {
        let class_fqn = class_type.qualified_name();
        let members = engine.get_class_members_cached(&class_fqn);
        let matching_methods: Vec<_> = members
            .into_iter()
            .filter(|m| m.simple_name.as_ref() == method_name && m.kind == LspSymbolKind::METHOD)
            .collect();

        if matching_methods.is_empty() {
            debug!(
                "No method named '{}' found on type '{}'",
                method_name, class_fqn
            );
        } else if let Some(return_type_str) = &matching_methods[0].return_type {
            debug!(
                "Method {}::{} returns {}",
                class_fqn, method_name, return_type_str
            );
            if let Some(return_type) = engine.parse_type_string(return_type_str) {
                return return_type;
            }
            debug!("Failed to parse method return type: {}", return_type_str);
        }
    }

    Type::Unknown
}

/// Infer the type of a constructor reference expression
fn infer_constructor_reference(
    engine: &TypeInferenceEngine,
    node: &SyntaxNode,
    resolver: &SymbolResolver,
    expected_type: Option<&Type>,
) -> Type {
    debug!("Inferring constructor reference type");

    let receiver_text = extract_constructor_reference_receiver(node);

    let receiver_text = if let Some(r) = receiver_text {
        r
    } else {
        debug!("Failed to extract constructor reference receiver");
        return Type::Unknown;
    };

    debug!("Constructor reference: {}::new", receiver_text);

    // Check if it's an array constructor reference
    if receiver_text.ends_with("[]") {
        return engine
            .parse_type_string(&receiver_text)
            .unwrap_or(Type::Unknown);
    }

    // Resolve the receiver type
    let receiver_type = if let Some(symbol) = resolver.resolve_identifier(&receiver_text, node) {
        symbol.type_info
    } else if let Some(t) = engine.parse_type_string(&receiver_text) {
        t
    } else {
        debug!(
            "Failed to parse constructor receiver type: {}",
            receiver_text
        );
        return Type::Unknown;
    };

    debug!("Resolved constructor receiver type: {:?}", receiver_type);

    // Apply generic type inference from expected type if available
    if let (Type::Class(mut class_type), Some(expected)) = (receiver_type.clone(), expected_type) {
        if let Type::Class(expected_class) = expected {
            if !expected_class.type_arguments.is_empty() && class_type.type_arguments.is_empty() {
                debug!(
                    "Inferring type arguments from expected type: {:?}",
                    expected_class
                );

                if is_supplier_like(&expected_class.name) {
                    if let Some(Type::Class(return_type)) = expected_class.type_arguments.first() {
                        if !return_type.type_arguments.is_empty() {
                            class_type
                                .type_arguments
                                .clone_from(&return_type.type_arguments);
                            debug!(
                                "Inferred type arguments for {}: {:?}",
                                class_type.name, class_type.type_arguments
                            );
                        }
                    }
                }
            }
        }

        return Type::Class(class_type);
    }

    receiver_type
}

/// Extract the receiver text from a constructor reference's parent node
fn extract_constructor_reference_receiver(node: &SyntaxNode) -> Option<String> {
    let parent = node.parent()?;

    let mut receiver_parts = Vec::new();
    let mut found_our_node = false;

    for child in parent.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::COLON_COLON {
                    break;
                }
                match t.kind() {
                    SyntaxKind::IDENT
                    | SyntaxKind::DOT
                    | SyntaxKind::L_BRACKET
                    | SyntaxKind::R_BRACKET => {
                        receiver_parts.push(t.text().to_string());
                    }
                    _ => {}
                }
            }
            NodeOrToken::Node(n) => {
                if n.text_range() == node.text_range() {
                    found_our_node = true;
                    break;
                }
                let text = n.text().to_string();
                if !text.trim().is_empty() {
                    receiver_parts.push(text.trim().to_string());
                }
            }
        }
    }

    if receiver_parts.is_empty() && !found_our_node {
        // Fallback to text-based extraction
        let parent_text = parent.text().to_string();
        if let Some((left, _)) = parent_text.split_once("::") {
            let trimmed = left.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        return None;
    }

    let receiver = receiver_parts.join("");
    if receiver.is_empty() {
        None
    } else {
        Some(receiver)
    }
}

/// Check if a class name is a Supplier-like functional interface
fn is_supplier_like(class_name: &str) -> bool {
    matches!(
        class_name,
        "Supplier" | "Provider" | "Factory" | "Builder" | "Creator"
    )
}
