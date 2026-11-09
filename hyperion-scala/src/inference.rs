// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Local type inference for Scala.
//!
//! This module implements local type inference for Scala `val` and `var` bindings.
//! It handles simple cases:
//! - Literals: `val x = 42` → `Int`
//! - Constructors: `val u = new User()` → `User`
//! - Method calls: `val s = obj.method()` → return type of method
//! - Generic method calls: `val l = List(1, 2, 3)` → `List[Int]` (basic unification)
//!
//! This is NOT full Hindley-Milner inference. We don't attempt to infer:
//! - Complex implicit-dependent types
//! - Higher-kinded type inference
//! - Path-dependent types
//! - Type inference across multiple files
//!
//! For unsupported cases, we return `Unknown` type.

use crate::parser::NodeExt;
use std::collections::HashMap;
use tracing::debug;
use tree_sitter::Node;

/// A simple representation of Scala types for inference
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InferredType {
    /// Named type: "String", "Int", "User"
    Named {
        simple_name: String,
        type_args: Vec<Self>,
    },

    /// Type variable (for generics): "T", "A"
    TypeVar { name: String },

    /// Function type: "(Int, String) => Boolean"
    Function {
        params: Vec<Self>,
        return_type: Box<Self>,
    },

    /// Tuple type: "(Int, String)"
    Tuple { elements: Vec<Self> },

    /// Unknown type (inference failed)
    Unknown,
}

impl InferredType {
    /// Create a simple named type
    pub fn simple(name: &str) -> Self {
        Self::Named {
            simple_name: name.to_string(),
            type_args: Vec::new(),
        }
    }

    /// Create a generic type with type arguments
    pub fn generic(name: &str, args: Vec<Self>) -> Self {
        Self::Named {
            simple_name: name.to_string(),
            type_args: args,
        }
    }

    /// Create a function type
    pub fn function(params: Vec<Self>, return_type: Self) -> Self {
        Self::Function {
            params,
            return_type: Box::new(return_type),
        }
    }
}

impl std::fmt::Display for InferredType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named {
                simple_name,
                type_args,
            } => {
                if type_args.is_empty() {
                    write!(f, "{simple_name}")
                } else {
                    let args_str = type_args
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(f, "{simple_name}[{args_str}]")
                }
            }
            Self::TypeVar { name } => write!(f, "{name}"),
            Self::Function {
                params,
                return_type,
            } => {
                let params_str = if params.len() == 1 {
                    params[0].to_string()
                } else {
                    let p = params
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("({p})")
                };
                write!(f, "{params_str} => {return_type}")
            }
            Self::Tuple { elements } => {
                let elems_str = elements
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({elems_str})")
            }
            Self::Unknown => write!(f, "?"),
        }
    }
}

/// Type unifier for generic type inference
#[derive(Debug, Default)]
pub struct TypeUnifier {
    /// Mapping from type variable names to inferred concrete types
    substitutions: HashMap<String, InferredType>,
}

impl TypeUnifier {
    /// Create a new type unifier
    pub fn new() -> Self {
        Self {
            substitutions: HashMap::new(),
        }
    }

    /// Attempt to unify an expected type with an actual type
    ///
    /// This binds type variables in the expected type to concrete types from actual.
    /// Returns true if unification succeeded.
    pub fn unify(&mut self, expected: &InferredType, actual: &InferredType) -> bool {
        match (expected, actual) {
            // Type variable unification
            (InferredType::TypeVar { name }, actual_type) => {
                if let Some(existing) = self.substitutions.get(name) {
                    // Already bound - check compatibility
                    Self::types_compatible(existing, actual_type)
                } else {
                    // Bind type variable to concrete type
                    self.substitutions.insert(name.clone(), actual_type.clone());
                    true
                }
            }

            // Named type unification
            (
                InferredType::Named {
                    simple_name: exp_name,
                    type_args: exp_args,
                },
                InferredType::Named {
                    simple_name: act_name,
                    type_args: act_args,
                },
            ) => {
                // Names must match
                if exp_name != act_name {
                    return false;
                }

                // Type arguments must have same length
                if exp_args.len() != act_args.len() {
                    return false;
                }

                // Recursively unify type arguments
                exp_args
                    .iter()
                    .zip(act_args.iter())
                    .all(|(e, a)| self.unify(e, a))
            }

            // Function type unification
            (
                InferredType::Function {
                    params: exp_params,
                    return_type: exp_ret,
                },
                InferredType::Function {
                    params: act_params,
                    return_type: act_ret,
                },
            ) => {
                if exp_params.len() != act_params.len() {
                    return false;
                }

                // Unify parameters
                let params_match = exp_params
                    .iter()
                    .zip(act_params.iter())
                    .all(|(e, a)| self.unify(e, a));

                // Unify return type
                params_match && self.unify(exp_ret, act_ret)
            }

            // Tuple type unification
            (
                InferredType::Tuple {
                    elements: exp_elems,
                },
                InferredType::Tuple {
                    elements: act_elems,
                },
            ) => {
                if exp_elems.len() != act_elems.len() {
                    return false;
                }

                exp_elems
                    .iter()
                    .zip(act_elems.iter())
                    .all(|(e, a)| self.unify(e, a))
            }

            // Unknown types always unify (permissive)
            (InferredType::Unknown, _) | (_, InferredType::Unknown) => true,

            // Otherwise, types don't match
            _ => false,
        }
    }

    /// Check if two types are compatible (simplified)
    fn types_compatible(a: &InferredType, b: &InferredType) -> bool {
        a == b || matches!(a, InferredType::Unknown) || matches!(b, InferredType::Unknown)
    }

    /// Apply substitutions to a type
    pub fn apply(&self, ty: &InferredType) -> InferredType {
        match ty {
            InferredType::TypeVar { name } => self
                .substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| ty.clone()),
            InferredType::Named {
                simple_name,
                type_args,
            } => InferredType::Named {
                simple_name: simple_name.clone(),
                type_args: type_args.iter().map(|t| self.apply(t)).collect(),
            },
            InferredType::Function {
                params,
                return_type,
            } => InferredType::Function {
                params: params.iter().map(|t| self.apply(t)).collect(),
                return_type: Box::new(self.apply(return_type)),
            },
            InferredType::Tuple { elements } => InferredType::Tuple {
                elements: elements.iter().map(|t| self.apply(t)).collect(),
            },
            InferredType::Unknown => InferredType::Unknown,
        }
    }

    /// Get the substitutions map
    pub const fn substitutions(&self) -> &HashMap<String, InferredType> {
        &self.substitutions
    }
}

/// Infer type from a Scala expression node
///
/// This is the main entry point for type inference.
pub fn infer_expression_type(node: &Node, source: &str) -> InferredType {
    match node.kind() {
        // Literals
        "integer_literal" => InferredType::simple("Int"),
        "floating_point_literal" => InferredType::simple("Double"),
        "boolean_literal" => InferredType::simple("Boolean"),
        "string" | "interpolated_string" => InferredType::simple("String"),
        "character_literal" => InferredType::simple("Char"),
        "null_literal" => InferredType::simple("Null"),

        // Instance creation: new User()
        "instance_expression" => infer_instance_creation(node, source),

        // Method call: obj.method(), or apply-style construction: List(1, 2, 3)
        //
        // tree-sitter-scala (0.24) represents both `obj.method()` and `List(1, 2, 3)`
        // as "call_expression" nodes; it has no distinct "apply" node kind. Try the
        // apply-style heuristics first (they only match known collection
        // constructors) and fall back to the (currently unimplemented) general
        // method-call resolution otherwise.
        "call_expression" => {
            let apply_result = infer_apply_expression(node, source);
            if matches!(apply_result, InferredType::Unknown) {
                infer_method_call(node, source)
            } else {
                apply_result
            }
        }

        // Apply: List(1, 2, 3) or obj(args)
        // Kept for forward-compatibility in case a future grammar version
        // reintroduces a distinct "apply" node kind.
        "apply" => infer_apply_expression(node, source),

        // Identifier: variable reference
        "identifier" => {
            // We can't infer the type without symbol table lookup
            // Return unknown for now
            InferredType::Unknown
        }

        // Tuple: (1, "hello", true)
        "tuple_expression" => infer_tuple(node, source),

        // Lambda/anonymous function: (x: Int) => x + 1
        "lambda_expression" | "function_expression" => infer_lambda(node, source),

        // If expression: if (cond) a else b
        "if_expression" => infer_if_expression(node, source),

        // Match expression: x match { case ... }
        "match_expression" => {
            // Would need to analyze all branches
            InferredType::Unknown
        }

        // Block: { ... }
        "block" | "template_body" => infer_block(node, source),

        _ => {
            debug!("Cannot infer type for node kind: {}", node.kind());
            InferredType::Unknown
        }
    }
}

/// Infer type from instance creation: new User()
fn infer_instance_creation(node: &Node, source: &str) -> InferredType {
    // Look for the type being instantiated
    // Structure: instance_expression -> simple_type or generic_type
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return InferredType::Unknown;
    }

    // Skip "new" keyword
    loop {
        let child = cursor.node();
        if child.kind() == "simple_type" || child.kind() == "generic_type" {
            return extract_type_from_node(&child, source);
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }

    InferredType::Unknown
}

/// Infer type from method call: obj.method()
const fn infer_method_call(_node: &Node, _source: &str) -> InferredType {
    // Without symbol table, we can't resolve the method's return type
    // Return Unknown for now
    // In a full implementation, we'd:
    // 1. Infer the type of the receiver (obj)
    // 2. Look up the method in the receiver's type
    // 3. Return the method's return type
    InferredType::Unknown
}

/// Infer type from apply expression: List(1, 2, 3)
fn infer_apply_expression(node: &Node, source: &str) -> InferredType {
    // Only a call of the collection itself is a constructor. A call on its result, such as
    // `List(1, 2).mkString(",")`, has the same text prefix and is not one.
    let Some(callee) = node.child_by_field("function").or_else(|| node.child(0)) else {
        return InferredType::Unknown;
    };
    match callee.text(source) {
        "List" => {
            let arg_types = extract_argument_types(node, source);
            if arg_types.is_empty() {
                // Empty list - List[Nothing] in Scala, but we'll use Unknown
                return InferredType::generic("List", vec![InferredType::Unknown]);
            }
            InferredType::generic("List", vec![least_upper_bound(&arg_types)])
        }
        "Set" => {
            let arg_types = extract_argument_types(node, source);
            let elem_type = least_upper_bound(&arg_types);
            InferredType::generic("Set", vec![elem_type])
        }
        // Map needs key and value types
        // For now, return Map[Unknown, Unknown]
        "Map" => InferredType::generic("Map", vec![InferredType::Unknown, InferredType::Unknown]),
        _ => InferredType::Unknown,
    }
}

/// The least upper bound of the argument types of a collection literal, as Scala gives
/// the element type of `List(1, "a")`. An empty list has no bound.
fn least_upper_bound(types: &[InferredType]) -> InferredType {
    let mut iter = types.iter();
    let Some(first) = iter.next() else {
        return InferredType::Unknown;
    };
    iter.fold(first.clone(), |acc, ty| lub_pair(&acc, ty))
}

/// The least upper bound of two types. Numeric types widen to the wider one, `null` takes
/// the other reference type, and any other mismatch is `Any`. An unknown type stays unknown.
fn lub_pair(a: &InferredType, b: &InferredType) -> InferredType {
    if a == b {
        return a.clone();
    }
    if matches!(a, InferredType::Unknown) || matches!(b, InferredType::Unknown) {
        return InferredType::Unknown;
    }
    if let (Some(rank_a), Some(rank_b)) = (numeric_rank(a), numeric_rank(b)) {
        return if rank_a >= rank_b {
            a.clone()
        } else {
            b.clone()
        };
    }
    if is_null_type(a) {
        return if is_value_type(b) {
            InferredType::simple("Any")
        } else {
            b.clone()
        };
    }
    if is_null_type(b) {
        return if is_value_type(a) {
            InferredType::simple("Any")
        } else {
            a.clone()
        };
    }
    InferredType::simple("Any")
}

/// Scala's numeric widening order: Int, Long, Float, Double.
fn numeric_rank(ty: &InferredType) -> Option<u8> {
    match ty {
        InferredType::Named {
            simple_name,
            type_args,
        } if type_args.is_empty() => match simple_name.as_str() {
            "Int" => Some(0),
            "Long" => Some(1),
            "Float" => Some(2),
            "Double" => Some(3),
            _ => None,
        },
        _ => None,
    }
}

/// Whether a type is the type of `null`.
fn is_null_type(ty: &InferredType) -> bool {
    matches!(ty, InferredType::Named { simple_name, type_args } if simple_name == "Null" && type_args.is_empty())
}

/// Whether a type is a Scala value type, which `null` cannot be.
fn is_value_type(ty: &InferredType) -> bool {
    numeric_rank(ty).is_some()
        || matches!(
            ty,
            InferredType::Named { simple_name, type_args }
                if type_args.is_empty() && matches!(simple_name.as_str(), "Boolean" | "Char")
        )
}

/// Extract types of all arguments in an apply expression
fn extract_argument_types(node: &Node, source: &str) -> Vec<InferredType> {
    let mut types = Vec::new();
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return types;
    }

    loop {
        let child = cursor.node();

        // Look for argument_exprs or similar
        if child.kind() == "argument_exprs" || child.kind() == "arguments" {
            // Extract types from each argument
            let mut arg_cursor = child.walk();
            if arg_cursor.goto_first_child() {
                loop {
                    let arg = arg_cursor.node();
                    // Skip punctuation
                    if arg.kind() != "(" && arg.kind() != ")" && arg.kind() != "," {
                        types.push(infer_expression_type(&arg, source));
                    }
                    if !arg_cursor.goto_next_sibling() {
                        break;
                    }
                }
            }
            break;
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    types
}

/// Infer tuple type: (1, "hello", true)
fn infer_tuple(node: &Node, source: &str) -> InferredType {
    let mut element_types = Vec::new();
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return InferredType::Unknown;
    }

    loop {
        let child = cursor.node();

        // Skip punctuation
        if child.kind() != "(" && child.kind() != ")" && child.kind() != "," {
            element_types.push(infer_expression_type(&child, source));
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    if element_types.is_empty() {
        InferredType::Unknown
    } else {
        InferredType::Tuple {
            elements: element_types,
        }
    }
}

/// Infer lambda type: (x: Int) => x + 1
const fn infer_lambda(_node: &Node, _source: &str) -> InferredType {
    // Extract parameter types and return type
    // For now, return Unknown
    // Full implementation would:
    // 1. Extract parameter types from annotations
    // 2. Infer return type from body
    InferredType::Unknown
}

/// Infer type from if expression: if (cond) a else b
fn infer_if_expression(node: &Node, source: &str) -> InferredType {
    // Without an else branch the if is Unit. With one, both branches give the type.
    let Some(consequence) = node.child_by_field("consequence") else {
        return InferredType::Unknown;
    };
    let then_type = infer_expression_type(&consequence, source);
    match node.child_by_field("alternative") {
        Some(alternative) => lub_pair(&then_type, &infer_expression_type(&alternative, source)),
        None => InferredType::simple("Unit"),
    }
}

/// Infer type from block: { val x = 1; x + 1 }
fn infer_block(node: &Node, source: &str) -> InferredType {
    // The type of a block is the type of its last expression
    let mut last_expr = None;
    let mut cursor = node.walk();

    if !cursor.goto_first_child() {
        return InferredType::Unknown;
    }

    loop {
        let child = cursor.node();

        // Track any expression node
        if is_expression_node(&child) {
            last_expr = Some(child);
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }

    last_expr.map_or_else(
        || InferredType::simple("Unit"),
        |expr| infer_expression_type(&expr, source),
    )
}

/// Check if a node represents an expression
fn is_expression_node(node: &Node) -> bool {
    matches!(
        node.kind(),
        "call_expression"
            | "apply"
            | "instance_expression"
            | "if_expression"
            | "match_expression"
            | "integer_literal"
            | "floating_point_literal"
            | "boolean_literal"
            | "string"
            | "identifier"
            | "tuple_expression"
            | "lambda_expression"
            | "function_expression"
            | "block"
    )
}

/// Extract type from a type node (simple_type or generic_type)
fn extract_type_from_node(node: &Node, source: &str) -> InferredType {
    match node.kind() {
        "simple_type" | "type_identifier" => {
            let name = node.text(source);
            InferredType::simple(name)
        }
        "generic_type" => {
            // Extract base type and type arguments
            let mut cursor = node.walk();

            if !cursor.goto_first_child() {
                return InferredType::Unknown;
            }

            let mut base_name = None;
            let mut type_args = Vec::new();

            loop {
                let child = cursor.node();

                if base_name.is_none()
                    && (child.kind() == "type_identifier" || child.kind() == "simple_type")
                {
                    base_name = Some(child.text(source).to_string());
                } else if child.kind() == "type_arguments" {
                    // Extract type arguments
                    let mut arg_cursor = child.walk();
                    if arg_cursor.goto_first_child() {
                        loop {
                            let arg = arg_cursor.node();
                            if arg.kind() != "[" && arg.kind() != "]" && arg.kind() != "," {
                                type_args.push(extract_type_from_node(&arg, source));
                            }
                            if !arg_cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                }

                if !cursor.goto_next_sibling() {
                    break;
                }
            }

            base_name.map_or(InferredType::Unknown, |name| {
                InferredType::generic(&name, type_args)
            })
        }
        _ => InferredType::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn infer_val_type(source: &str) -> InferredType {
        let parse_result = parse(source).expect("Parse failed");
        let root = parse_result.tree.root_node();

        // Find the val_definition node
        let mut cursor = root.walk();
        if cursor.goto_first_child() {
            loop {
                let node = cursor.node();
                if node.kind() == "val_definition" || node.kind() == "value_definition" {
                    // Use the grammar's `value` field directly rather than scanning
                    // children for "anything expression-shaped": the `pattern` field
                    // (e.g. the bound identifier `x` in `val x = 42`) is itself an
                    // `identifier` node, which `is_expression_node` also matches, so a
                    // naive first-match scan picks up the binding name instead of the
                    // initializer.
                    if let Some(child) = node.child_by_field_name("value") {
                        return infer_expression_type(&child, source);
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }

        InferredType::Unknown
    }

    #[test]
    fn test_infer_int_literal() {
        let ty = infer_val_type("val x = 42");
        assert_eq!(ty, InferredType::simple("Int"));
    }

    #[test]
    fn test_infer_string_literal() {
        let ty = infer_val_type("val s = \"hello\"");
        assert_eq!(ty, InferredType::simple("String"));
    }

    #[test]
    fn test_infer_boolean_literal() {
        let ty = infer_val_type("val b = true");
        assert_eq!(ty, InferredType::simple("Boolean"));
    }

    #[test]
    fn test_infer_double_literal() {
        let ty = infer_val_type("val d = 3.14");
        assert_eq!(ty, InferredType::simple("Double"));
    }

    #[test]
    fn test_infer_list_with_ints() {
        let ty = infer_val_type("val list = List(1, 2, 3)");
        assert_eq!(
            ty,
            InferredType::generic("List", vec![InferredType::simple("Int")])
        );
    }

    #[test]
    fn test_type_unifier_simple() {
        let mut unifier = TypeUnifier::new();

        let expected = InferredType::TypeVar {
            name: "T".to_string(),
        };
        let actual = InferredType::simple("Int");

        assert!(unifier.unify(&expected, &actual));
        assert_eq!(
            unifier.substitutions().get("T"),
            Some(&InferredType::simple("Int"))
        );
    }

    #[test]
    fn test_type_unifier_generic() {
        let mut unifier = TypeUnifier::new();

        let expected = InferredType::generic(
            "List",
            vec![InferredType::TypeVar {
                name: "T".to_string(),
            }],
        );
        let actual = InferredType::generic("List", vec![InferredType::simple("String")]);

        assert!(unifier.unify(&expected, &actual));
        assert_eq!(
            unifier.substitutions().get("T"),
            Some(&InferredType::simple("String"))
        );
    }

    #[test]
    fn test_list_of_mixed_types_is_any() {
        let ty = infer_val_type("val xs = List(1, \"a\")");
        assert_eq!(
            ty,
            InferredType::generic("List", vec![InferredType::simple("Any")])
        );
    }

    #[test]
    fn test_list_of_int_and_double_widens_to_double() {
        let ty = infer_val_type("val xs = List(1, 2.0)");
        assert_eq!(
            ty,
            InferredType::generic("List", vec![InferredType::simple("Double")])
        );
    }

    #[test]
    fn test_list_of_null_and_string_is_string() {
        let ty = infer_val_type("val xs = List(null, \"a\")");
        assert_eq!(
            ty,
            InferredType::generic("List", vec![InferredType::simple("String")])
        );
    }

    #[test]
    fn test_a_method_on_a_list_is_not_the_list() {
        // The call's function is `List(...).mkString`, not `List`, so it is not a constructor
        let ty = infer_val_type("val s = List(\"a\", \"b\").mkString(\",\")");
        assert_eq!(ty, InferredType::Unknown);
    }

    #[test]
    fn test_if_with_an_else_is_the_common_type_of_both_branches() {
        assert_eq!(
            infer_val_type("val x = if (flag) 1 else 2"),
            InferredType::simple("Int")
        );
        assert_eq!(
            infer_val_type("val x = if (flag) 1 else \"a\""),
            InferredType::simple("Any")
        );
    }

    #[test]
    fn test_if_without_an_else_is_unit() {
        assert_eq!(
            infer_val_type("val x = if (flag) 1"),
            InferredType::simple("Unit")
        );
    }

    #[test]
    fn test_infer_tuple() {
        let ty = infer_val_type("val t = (1, \"hello\", true)");
        assert_eq!(
            ty,
            InferredType::Tuple {
                elements: vec![
                    InferredType::simple("Int"),
                    InferredType::simple("String"),
                    InferredType::simple("Boolean")
                ]
            }
        );
    }
}
