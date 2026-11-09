// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Argument Type Inference for Method Overload Resolution
//!
//! This module provides utilities for inferring the types of arguments at method call sites,
//! enabling proper overload resolution when multiple methods have the same parameter count
//! but different parameter types.

use hyperion_parser::syntax::SyntaxKind;
use hyperion_parser::JavaLanguage;
use hyperion_semantics::db::FileId;
use hyperion_semantics::file_semantics::FileSemantics;
use hyperion_semantics::inference::TypeInferenceEngine;
use hyperion_semantics::resolver::{ResolverBuilder, SymbolResolver};
use hyperion_semantics::symbol::{ClassType, PrimitiveType, Type};

use hyperion_workspace::WorkspaceIndex;
use rowan::SyntaxNode;
use std::cell::OnceCell;
use std::sync::Arc;
use tracing::debug;

/// Bundled context for argument type inference.
///
/// Groups the parameters that every inference function needs, eliminating
/// the 5-parameter threading pattern (`file_id, workspace_index, syntax_root,
/// offset, file_sem`) across the call chain.
pub struct InferenceContext<'a> {
    pub file_id: FileId,
    pub workspace_index: &'a Arc<WorkspaceIndex>,
    pub syntax_root: &'a SyntaxNode<JavaLanguage>,
    pub offset: u32,
    pub file_sem: Option<&'a FileSemantics>,
    resolver: OnceCell<SymbolResolver>,
    engine: OnceCell<TypeInferenceEngine>,
}

/// Confidence level for inferred types
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// Type could not be inferred
    Unknown,
    /// Type inferred from context (variable lookup, method return)
    Likely,
    /// Type is certain (literals, new expressions, casts)
    Certain,
}

/// An inferred type with confidence level
#[derive(Debug, Clone)]
pub struct InferredType {
    pub ty: Type,
    pub confidence: Confidence,
}

impl InferredType {
    pub const fn certain(ty: Type) -> Self {
        Self {
            ty,
            confidence: Confidence::Certain,
        }
    }

    pub const fn likely(ty: Type) -> Self {
        Self {
            ty,
            confidence: Confidence::Likely,
        }
    }

    pub const fn unknown() -> Self {
        Self {
            ty: Type::Unknown,
            confidence: Confidence::Unknown,
        }
    }
}

/// Infer types for all arguments in an ARG_LIST node
///
/// Returns a vector of inferred types, one for each argument expression.
/// The order matches the order of arguments in the call.
///
/// Uses comma-based boundaries to correctly count arguments even when
/// expressions contain nested sub-expressions (e.g., `method(a.b())` is 1 arg, not 2).
pub fn infer_argument_types(
    arg_list: &SyntaxNode<JavaLanguage>,
    file_id: FileId,
    workspace_index: &Arc<WorkspaceIndex>,
    syntax_root: &SyntaxNode<JavaLanguage>,
    offset: u32,
) -> Vec<InferredType> {
    let ctx = InferenceContext {
        file_id,
        workspace_index,
        syntax_root,
        offset,
        file_sem: None,
        resolver: OnceCell::new(),
        engine: OnceCell::new(),
    };
    infer_argument_types_impl(arg_list, &ctx)
}

/// Infer argument types with optional FileSemantics fast path.
///
/// When `file_sem` is provided, cached expression types are checked first,
/// avoiding redundant AST-based inference for expressions already resolved
/// by the semantic walker.
pub fn infer_argument_types_with_semantics(
    arg_list: &SyntaxNode<JavaLanguage>,
    file_id: FileId,
    workspace_index: &Arc<WorkspaceIndex>,
    syntax_root: &SyntaxNode<JavaLanguage>,
    offset: u32,
    file_sem: Option<&FileSemantics>,
) -> Vec<InferredType> {
    let ctx = InferenceContext {
        file_id,
        workspace_index,
        syntax_root,
        offset,
        file_sem,
        resolver: OnceCell::new(),
        engine: OnceCell::new(),
    };
    infer_argument_types_impl(arg_list, &ctx)
}

fn infer_argument_types_impl(
    arg_list: &SyntaxNode<JavaLanguage>,
    ctx: &InferenceContext,
) -> Vec<InferredType> {
    let mut arg_types = Vec::new();

    // Use comma-based argument extraction to correctly handle nested expressions
    let mut current_arg_first_expr: Option<SyntaxNode<JavaLanguage>> = None;

    for child in arg_list.children_with_tokens() {
        match child {
            // Record only the first expression node for this argument.
            rowan::NodeOrToken::Node(node)
                if current_arg_first_expr.is_none() && is_expression_kind(node.kind()) =>
            {
                current_arg_first_expr = Some(node.clone());
            }
            rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMA => {
                // Comma marks end of current argument - process it
                if let Some(expr) = current_arg_first_expr.take() {
                    let inferred = infer_expression_type(&expr, ctx);
                    debug!(
                        "Inferred argument type: {:?} (confidence: {:?})",
                        inferred.ty, inferred.confidence
                    );
                    arg_types.push(inferred);
                }
                // Reset for next argument
                current_arg_first_expr = None;
            }
            _ => {
                // Skip other tokens (parens, whitespace, etc.)
            }
        }
    }

    // Don't forget the last argument (no trailing comma)
    if let Some(expr) = current_arg_first_expr.take() {
        let inferred = infer_expression_type(&expr, ctx);
        debug!(
            "Inferred argument type: {:?} (confidence: {:?})",
            inferred.ty, inferred.confidence
        );
        arg_types.push(inferred);
    }

    arg_types
}

pub(crate) fn infer_expression_type_for_navigation(
    expr: &SyntaxNode<JavaLanguage>,
    file_id: FileId,
    workspace_index: &Arc<WorkspaceIndex>,
    syntax_root: &SyntaxNode<JavaLanguage>,
    offset: u32,
) -> InferredType {
    let ctx = InferenceContext {
        file_id,
        workspace_index,
        syntax_root,
        offset,
        file_sem: None,
        resolver: OnceCell::new(),
        engine: OnceCell::new(),
    };
    infer_expression_type(expr, &ctx)
}

/// Infer the type of a single expression node
fn infer_expression_type(expr: &SyntaxNode<JavaLanguage>, ctx: &InferenceContext) -> InferredType {
    // Fast path: check pre-computed expression types from FileSemantics
    if let Some(sem) = ctx.file_sem {
        if let Some(cached_type) = sem.expression_type_at(expr.text_range()) {
            let confidence = match cached_type {
                Type::Unknown => Confidence::Unknown,
                _ => Confidence::Certain,
            };
            return InferredType {
                ty: cached_type.clone(),
                confidence,
            };
        }
    }
    match expr.kind() {
        // Literals have certain types
        SyntaxKind::LITERAL_EXPR => infer_literal_type(expr),

        // null literal
        SyntaxKind::NULL_LITERAL => InferredType::certain(Type::Null),

        // new expressions: new ClassName(...)
        SyntaxKind::NEW_EXPR => infer_new_expr_type(expr, ctx.file_id, ctx.workspace_index),

        // Cast expressions: (Type) expr
        SyntaxKind::CAST_EXPR => infer_cast_type(expr, ctx.file_id, ctx.workspace_index),

        // Every class literal expression has the runtime type java.lang.Class.
        // Keeping this distinct from the referenced type is essential for
        // overloads such as `isInstanceOf(Class<?>)`.
        SyntaxKind::CLASS_LITERAL_EXPR => InferredType::certain(Type::Class(ClassType::new(
            "Class".to_string(),
            Some("java.lang".to_string()),
        ))),

        // this keyword
        SyntaxKind::THIS_EXPR => {
            infer_this_type(ctx.syntax_root, u32::from(expr.text_range().start()))
        }

        // Variable references
        SyntaxKind::NAME_EXPR => infer_name_expr_type(expr, ctx),

        // Parenthesized expressions: (expr)
        SyntaxKind::PAREN_EXPR => {
            // Recurse into the inner expression
            for child in expr.children() {
                if is_expression_kind(child.kind()) {
                    return infer_expression_type(&child, ctx);
                }
            }
            InferredType::unknown()
        }

        // Array initializer: { elem1, elem2, ... }
        SyntaxKind::ARRAY_INIT_EXPR => InferredType::unknown(), // Would need context to determine element type

        // Ternary: cond ? a : b - infer from one branch
        SyntaxKind::TERNARY_EXPR => infer_ternary_type(expr, ctx),

        // Binary expressions - depends on operator
        SyntaxKind::BINARY_EXPR => infer_binary_expr_type(expr, ctx),

        // Unary expressions
        SyntaxKind::UNARY_EXPR => infer_unary_expr_type(expr, ctx),

        SyntaxKind::METHOD_CALL_EXPR
        | SyntaxKind::FIELD_ACCESS_EXPR
        | SyntaxKind::ARRAY_ACCESS_EXPR => {
            let resolver = ctx.resolver.get_or_init(|| {
                ResolverBuilder::with_workspace(ctx.file_id, Arc::clone(ctx.workspace_index))
                    .build_from_syntax(ctx.syntax_root)
            });
            let engine = ctx.engine.get_or_init(|| {
                TypeInferenceEngine::new(Arc::clone(ctx.workspace_index), ctx.file_id)
            });
            let ty = engine.infer_expression(expr, resolver, None);
            if matches!(ty, Type::Unknown) {
                InferredType::unknown()
            } else {
                InferredType::likely(ty)
            }
        }

        // Lambda - would need functional interface inference
        SyntaxKind::LAMBDA_EXPR => InferredType::unknown(),

        // Method reference - would need functional interface inference
        SyntaxKind::METHOD_REF_EXPR => InferredType::unknown(),

        _ => InferredType::unknown(),
    }
}

/// Infer type from a literal expression
fn infer_literal_type(expr: &SyntaxNode<JavaLanguage>) -> InferredType {
    for child in expr.children_with_tokens() {
        if let Some(token) = child.as_token() {
            let ty = match token.kind() {
                SyntaxKind::INT_LITERAL => Type::Primitive(PrimitiveType::Int),
                SyntaxKind::LONG_LITERAL => Type::Primitive(PrimitiveType::Long),
                SyntaxKind::FLOAT_LITERAL => Type::Primitive(PrimitiveType::Float),
                SyntaxKind::DOUBLE_LITERAL => Type::Primitive(PrimitiveType::Double),
                SyntaxKind::CHAR_LITERAL => Type::Primitive(PrimitiveType::Char),
                SyntaxKind::BOOLEAN_LITERAL => Type::Primitive(PrimitiveType::Boolean),
                SyntaxKind::STRING_LITERAL | SyntaxKind::TEXT_BLOCK_LITERAL => {
                    Type::Class(ClassType {
                        name: "String".to_string(),
                        package: Some("java.lang".to_string()),
                        type_arguments: vec![],
                        is_diamond: false,
                    })
                }
                SyntaxKind::NULL_LITERAL => Type::Null,
                _ => continue,
            };
            return InferredType::certain(ty);
        }
    }
    InferredType::unknown()
}

/// Array dimensions declared by an array creation expression (JLS 15.10.1).
///
/// The parser writes `new T[] { .. }` as an `ARRAY_TYPE` node per bracket pair
/// after the element type, and `new T[3]` as bracket tokens directly under the
/// `NEW_EXPR`. Both forms create an array, so both are counted.
fn array_creation_dimensions(expr: &SyntaxNode<JavaLanguage>) -> u8 {
    let bracketed_nodes = expr
        .children()
        .filter(|child| child.kind() == SyntaxKind::ARRAY_TYPE)
        .count();
    let bracket_tokens = expr
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() == SyntaxKind::L_BRACKET)
        .count();
    u8::try_from(bracketed_nodes + bracket_tokens).unwrap_or(u8::MAX)
}

/// Wrap `element` in an array type if the `NEW_EXPR` creates one.
fn apply_array_creation(expr: &SyntaxNode<JavaLanguage>, element: Type) -> Type {
    match array_creation_dimensions(expr) {
        0 => element,
        dimensions => Type::Array {
            element_type: Box::new(element),
            dimensions,
        },
    }
}

/// Infer type from a new expression
///
/// An array creation expression has an *array* type (JLS 15.10.1):
/// `new Class<?>[] { .. }` is a `Class<?>[]`, not a `Class<?>`. Dropping the
/// dimensions made such an argument non-applicable to every `T[]` parameter,
/// which is why a call like `Proxy.newProxyInstance(loader, new Class<?>[]{..},
/// null)` matched no candidate.
fn infer_new_expr_type(
    expr: &SyntaxNode<JavaLanguage>,
    file_id: FileId,
    workspace_index: &Arc<WorkspaceIndex>,
) -> InferredType {
    // Look for the element type in NEW_EXPR
    // Structure: NEW_KW, CLASS_TYPE | PRIMITIVE_TYPE, [ARRAY_TYPE ..], ARG_LIST | ARRAY_INIT_EXPR
    for child in expr.children() {
        if child.kind() == SyntaxKind::PRIMITIVE_TYPE {
            if let Some(element) = parse_type_node_with_resolution(&child, file_id, workspace_index)
            {
                return InferredType::certain(apply_array_creation(expr, element));
            }
        }
        if child.kind() == SyntaxKind::CLASS_TYPE {
            if let Some(class_name) = extract_class_name_from_type(&child) {
                // Try to resolve the full class name
                let resolved = resolve_simple_class_name(&class_name, file_id, workspace_index);
                // Extract type arguments (e.g., <String> from ArrayList<String>)
                let type_arguments = extract_type_arguments(&child, file_id, workspace_index);
                // Check for diamond operator
                let is_diamond = type_arguments.is_empty() && has_diamond_operator(&child);
                let element = Type::Class(class_type_from_resolved_name(
                    resolved,
                    type_arguments,
                    is_diamond,
                ));
                return InferredType::certain(apply_array_creation(expr, element));
            }
        }
    }

    // Fallback: look for IDENT tokens directly
    for child in expr.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                let class_name = token.text().to_string();
                let resolved = resolve_simple_class_name(&class_name, file_id, workspace_index);
                return InferredType::certain(Type::Class(class_type_from_resolved_name(
                    resolved,
                    vec![],
                    false,
                )));
            }
        }
    }

    InferredType::unknown()
}

/// Check if a type node has a diamond operator <>
fn has_diamond_operator(node: &SyntaxNode<JavaLanguage>) -> bool {
    // Check if there's a TYPE_ARGS child with no actual type arguments
    for child in node.children() {
        if child.kind() == SyntaxKind::TYPE_ARGS {
            // If we have TYPE_ARGS but it contains only < and >, it's a diamond
            let has_type = child.children().any(|c| {
                matches!(
                    c.kind(),
                    SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::WILDCARD_TYPE
                )
            });
            return !has_type;
        }
    }
    false
}

/// Infer type from a cast expression
///
/// The cast's type name must be resolved to an FQN, exactly as `new` and
/// literal inference already do. `(String) obj` written under
/// `import java.lang.*` denotes `java.lang.String`, and overload matching
/// compares whole `Type`s: since 0a544619 ("fix(semantics): preserve package
/// identity in calls") an unqualified `String` no longer matches a parameter
/// declared `java.lang.String` -- deliberately, so that an unqualified name
/// cannot impersonate a same-named type from another package. This branch was
/// the one argument-inference producer still emitting `package: None`, so
/// every cast argument silently failed to match any candidate.
fn infer_cast_type(
    expr: &SyntaxNode<JavaLanguage>,
    file_id: FileId,
    workspace_index: &Arc<WorkspaceIndex>,
) -> InferredType {
    // Cast structure: L_PAREN, TYPE, R_PAREN, expr
    for child in expr.children() {
        let kind = child.kind();
        if kind == SyntaxKind::CLASS_TYPE || kind == SyntaxKind::PRIMITIVE_TYPE {
            if let Some(ty) = parse_type_node_with_resolution(&child, file_id, workspace_index) {
                return InferredType::certain(ty);
            }
        }
    }
    InferredType::unknown()
}

/// Infer type of 'this' keyword
fn infer_this_type(syntax_root: &SyntaxNode<JavaLanguage>, offset: u32) -> InferredType {
    let class_name = syntax_root
        .token_at_offset(rowan::TextSize::from(offset))
        .right_biased()
        .and_then(|token| token.parent())
        .and_then(|node| crate::navigation::ast_queries::find_enclosing_class(&node))
        .and_then(|node| crate::navigation::import_resolver::extract_class_name(&node));
    if let Some(class_name) = class_name {
        return InferredType::certain(Type::Class(ClassType {
            name: class_name,
            package: None, // Would need full resolution
            type_arguments: vec![],
            is_diamond: false,
        }));
    }
    InferredType::unknown()
}

/// Infer type from a name expression (variable reference)
fn infer_name_expr_type(expr: &SyntaxNode<JavaLanguage>, ctx: &InferenceContext) -> InferredType {
    // Extract the name
    let name = expr.text().to_string().trim().to_string();

    // Build resolver to look up variables
    let resolver = ctx.resolver.get_or_init(|| {
        ResolverBuilder::with_workspace(ctx.file_id, Arc::clone(ctx.workspace_index))
            .build_from_syntax(ctx.syntax_root)
    });

    // Resolve from the expression's exact lexical position.
    let expression_offset = u32::from(expr.text_range().start());
    let symbol = resolver
        .find_scope_at_offset(expression_offset)
        .and_then(|scope_id| {
            resolver.resolve_in_scope_at_offset(&name, scope_id, expression_offset)
        });

    if let Some(sym) = symbol {
        if sym.ty != Type::Unknown {
            return InferredType::likely(sym.ty);
        }
    }

    InferredType::unknown()
}

/// Infer type from a ternary expression (condition ? true_branch : false_branch)
fn infer_ternary_type(expr: &SyntaxNode<JavaLanguage>, ctx: &InferenceContext) -> InferredType {
    // The type is the common type of the two branches
    // For simplicity, just use the type of the first branch
    let mut exprs = Vec::new();
    for child in expr.children() {
        if is_expression_kind(child.kind()) {
            exprs.push(child);
        }
    }

    // exprs[0] is condition, exprs[1] is true branch, exprs[2] is false branch
    if exprs.len() >= 2 {
        return infer_expression_type(&exprs[1], ctx);
    }

    InferredType::unknown()
}

/// Infer type from a binary expression
fn infer_binary_expr_type(expr: &SyntaxNode<JavaLanguage>, ctx: &InferenceContext) -> InferredType {
    // Check operator type
    for child in expr.children_with_tokens() {
        if let Some(token) = child.as_token() {
            match token.kind() {
                // Comparison/relational operators return boolean
                SyntaxKind::EQ_EQ
                | SyntaxKind::BANG_EQ
                | SyntaxKind::LT
                | SyntaxKind::GT
                | SyntaxKind::LT_EQ
                | SyntaxKind::GT_EQ
                | SyntaxKind::AMP_AMP
                | SyntaxKind::PIPE_PIPE => {
                    return InferredType::certain(Type::Primitive(PrimitiveType::Boolean));
                }
                // String concatenation with +
                SyntaxKind::PLUS => {
                    // If either operand is String, result is String
                    let children: Vec<_> = expr.children().collect();
                    if children.len() >= 2 {
                        let left = infer_expression_type(&children[0], ctx);
                        if is_string_type(&left.ty) {
                            return InferredType::certain(Type::Class(ClassType {
                                name: "String".to_string(),
                                package: Some("java.lang".to_string()),
                                type_arguments: vec![],
                                is_diamond: false,
                            }));
                        }
                        let right = infer_expression_type(&children[1], ctx);
                        if is_string_type(&right.ty) {
                            return InferredType::certain(Type::Class(ClassType {
                                name: "String".to_string(),
                                package: Some("java.lang".to_string()),
                                type_arguments: vec![],
                                is_diamond: false,
                            }));
                        }
                        // Numeric addition - return wider type
                        return promote_numeric_types(&left.ty, &right.ty);
                    }
                }
                // Arithmetic operators
                SyntaxKind::MINUS | SyntaxKind::STAR | SyntaxKind::SLASH | SyntaxKind::PERCENT => {
                    let children: Vec<_> = expr.children().collect();
                    if children.len() >= 2 {
                        let left = infer_expression_type(&children[0], ctx);
                        let right = infer_expression_type(&children[1], ctx);
                        return promote_numeric_types(&left.ty, &right.ty);
                    }
                }
                // Bitwise operators on integers
                SyntaxKind::AMP | SyntaxKind::PIPE | SyntaxKind::CARET => {
                    // Bitwise ops return int or long
                    let children: Vec<_> = expr.children().collect();
                    if children.len() >= 2 {
                        let left = infer_expression_type(&children[0], ctx);
                        let right = infer_expression_type(&children[1], ctx);
                        return promote_numeric_types(&left.ty, &right.ty);
                    }
                }
                _ => continue,
            }
        }
    }

    InferredType::unknown()
}

/// Infer type from a unary expression
fn infer_unary_expr_type(expr: &SyntaxNode<JavaLanguage>, ctx: &InferenceContext) -> InferredType {
    // Check operator
    for child in expr.children_with_tokens() {
        if let Some(token) = child.as_token() {
            match token.kind() {
                // Logical NOT
                SyntaxKind::BANG => {
                    return InferredType::certain(Type::Primitive(PrimitiveType::Boolean));
                }
                // Bitwise NOT, unary minus/plus
                SyntaxKind::TILDE | SyntaxKind::MINUS | SyntaxKind::PLUS => {
                    // Get operand type
                    for child in expr.children() {
                        if is_expression_kind(child.kind()) {
                            return infer_expression_type(&child, ctx);
                        }
                    }
                }
                // Increment/decrement
                SyntaxKind::PLUS_PLUS | SyntaxKind::MINUS_MINUS => {
                    for child in expr.children() {
                        if is_expression_kind(child.kind()) {
                            return infer_expression_type(&child, ctx);
                        }
                    }
                }
                _ => continue,
            }
        }
    }

    InferredType::unknown()
}

// Helper functions

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
            | SyntaxKind::CLASS_LITERAL_EXPR
            | SyntaxKind::INSTANCEOF_EXPR
            | SyntaxKind::ASSIGNMENT_EXPR
    )
}

fn extract_class_name_from_type(type_node: &SyntaxNode<JavaLanguage>) -> Option<String> {
    // Look for class name in CLASS_TYPE node
    // Structure: CLASS_TYPE -> QUALIFIED_NAME -> IDENT tokens (for qualified names)
    // or: CLASS_TYPE -> IDENT (for simple names)
    for child in type_node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
        if let Some(node) = child.as_node() {
            if node.kind() == SyntaxKind::QUALIFIED_NAME {
                // Collect all IDENT and DOT tokens to form qualified name (e.g., java.util.ArrayList)
                let mut qualified_name = String::new();
                for c in node.children_with_tokens() {
                    if let Some(t) = c.as_token() {
                        match t.kind() {
                            SyntaxKind::IDENT => qualified_name.push_str(t.text()),
                            SyntaxKind::DOT => qualified_name.push('.'),
                            _ => {}
                        }
                    }
                }
                if !qualified_name.is_empty() {
                    return Some(qualified_name);
                }
            }
        }
    }
    None
}

fn resolve_simple_class_name(
    name: &str,
    file_id: FileId,
    workspace_index: &Arc<WorkspaceIndex>,
) -> String {
    // Try to resolve via imports
    use crate::navigation::import_resolver::resolve_class_fqn_via_imports;

    resolve_class_fqn_via_imports(workspace_index, file_id, name)
        .unwrap_or_else(|_| name.to_string())
}

/// Build a class type without storing the package twice.
///
/// Import resolution returns an FQN such as `com.example.Pair`; `ClassType`
/// stores that as the separate package `com.example` and simple name `Pair`.
/// Keeping the FQN in `name` as well produces the impossible identity
/// `com.example.com.example.Pair` when callers ask for `qualified_name()`.
fn class_type_from_resolved_name(
    resolved: String,
    type_arguments: Vec<Type>,
    is_diamond: bool,
) -> ClassType {
    let Some((package, name)) = resolved.rsplit_once('.') else {
        return ClassType {
            name: resolved,
            package: None,
            type_arguments,
            is_diamond,
        };
    };

    ClassType {
        name: name.to_string(),
        package: Some(package.to_string()),
        type_arguments,
        is_diamond,
    }
}

/// Extract type arguments from a CLASS_TYPE node (e.g., <String, Integer> from Map<String, Integer>)
fn extract_type_arguments(
    class_type: &SyntaxNode<JavaLanguage>,
    file_id: FileId,
    workspace_index: &Arc<WorkspaceIndex>,
) -> Vec<Type> {
    for child in class_type.children() {
        if child.kind() == SyntaxKind::TYPE_ARGS {
            return child
                .children()
                .filter_map(|arg| parse_type_node_with_resolution(&arg, file_id, workspace_index))
                .collect();
        }
    }
    vec![]
}

/// Parse a type node with full name resolution and recursive type argument extraction
fn parse_type_node_with_resolution(
    type_node: &SyntaxNode<JavaLanguage>,
    file_id: FileId,
    workspace_index: &Arc<WorkspaceIndex>,
) -> Option<Type> {
    match type_node.kind() {
        SyntaxKind::PRIMITIVE_TYPE => {
            let text = type_node.text().to_string();
            match text.trim() {
                "int" => Some(Type::Primitive(PrimitiveType::Int)),
                "long" => Some(Type::Primitive(PrimitiveType::Long)),
                "short" => Some(Type::Primitive(PrimitiveType::Short)),
                "byte" => Some(Type::Primitive(PrimitiveType::Byte)),
                "float" => Some(Type::Primitive(PrimitiveType::Float)),
                "double" => Some(Type::Primitive(PrimitiveType::Double)),
                "char" => Some(Type::Primitive(PrimitiveType::Char)),
                "boolean" => Some(Type::Primitive(PrimitiveType::Boolean)),
                _ => None,
            }
        }
        SyntaxKind::CLASS_TYPE => {
            let name = extract_class_name_from_type(type_node)?;
            let resolved = resolve_simple_class_name(&name, file_id, workspace_index);
            let type_arguments = extract_type_arguments(type_node, file_id, workspace_index);
            let is_diamond = type_arguments.is_empty() && has_diamond_operator(type_node);
            Some(Type::Class(class_type_from_resolved_name(
                resolved,
                type_arguments,
                is_diamond,
            )))
        }
        SyntaxKind::WILDCARD_TYPE => {
            // Handle ? extends/super bounds
            // For now, just return unbounded wildcard
            Some(Type::Wildcard {
                upper_bound: None,
                lower_bound: None,
            })
        }
        _ => None,
    }
}

fn is_string_type(ty: &Type) -> bool {
    matches!(ty, Type::Class(c) if c.name == "String")
}

fn promote_numeric_types(left: &Type, right: &Type) -> InferredType {
    // JLS §5.6.2 - Binary Numeric Promotion
    let get_rank = |ty: &Type| -> i32 {
        match ty {
            Type::Primitive(PrimitiveType::Double) => 4,
            Type::Primitive(PrimitiveType::Float) => 3,
            Type::Primitive(PrimitiveType::Long) => 2,
            Type::Primitive(
                PrimitiveType::Int
                | PrimitiveType::Short
                | PrimitiveType::Byte
                | PrimitiveType::Char,
            ) => 1,
            _ => 0,
        }
    };

    let left_rank = get_rank(left);
    let right_rank = get_rank(right);

    let result_type = match left_rank.max(right_rank) {
        4 => Type::Primitive(PrimitiveType::Double),
        3 => Type::Primitive(PrimitiveType::Float),
        2 => Type::Primitive(PrimitiveType::Long),
        1 => Type::Primitive(PrimitiveType::Int), // byte, short, char all promote to int
        _ => return InferredType::unknown(),
    };

    InferredType::certain(result_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyperion_parser::{Lexer, Parser};

    fn parse_expression(code: &str) -> SyntaxNode<JavaLanguage> {
        // Wrap in a class to make it valid Java
        let full_code = format!("class Test {{ void test() {{ Object x = {}; }} }}", code);
        let tokens = Lexer::new(&full_code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        SyntaxNode::new_root(green_node)
    }

    #[test]
    fn test_infer_int_literal() {
        let root = parse_expression("42");
        // Find the literal in the tree
        fn find_literal(node: &SyntaxNode<JavaLanguage>) -> Option<SyntaxNode<JavaLanguage>> {
            if node.kind() == SyntaxKind::LITERAL_EXPR {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_literal(&child) {
                    return Some(found);
                }
            }
            None
        }

        let literal = find_literal(&root).expect("Should find literal");
        let inferred = infer_literal_type(&literal);

        assert!(matches!(inferred.ty, Type::Primitive(PrimitiveType::Int)));
        assert_eq!(inferred.confidence, Confidence::Certain);
    }

    #[test]
    fn test_infer_string_literal() {
        let root = parse_expression("\"hello\"");
        fn find_literal(node: &SyntaxNode<JavaLanguage>) -> Option<SyntaxNode<JavaLanguage>> {
            if node.kind() == SyntaxKind::LITERAL_EXPR {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_literal(&child) {
                    return Some(found);
                }
            }
            None
        }

        let literal = find_literal(&root).expect("Should find literal");
        let inferred = infer_literal_type(&literal);

        assert!(matches!(inferred.ty, Type::Class(ref c) if c.name == "String"));
        assert_eq!(inferred.confidence, Confidence::Certain);
    }

    #[test]
    fn class_literal_argument_has_java_lang_class_type() {
        let code = "class Test { void test() { consume(String.class); } }";
        let tokens = Lexer::new(code).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let arg_list = root
            .descendants()
            .find(|node| {
                node.kind() == SyntaxKind::ARG_LIST && node.text().to_string().contains(".class")
            })
            .expect("class-literal argument list");
        let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp")));

        let inferred = infer_argument_types(
            &arg_list,
            FileId::new(0),
            &workspace_index,
            &root,
            u32::from(arg_list.text_range().start()),
        );

        assert_eq!(inferred.len(), 1);
        assert!(matches!(
            &inferred[0].ty,
            Type::Class(class)
                if class.package.as_deref() == Some("java.lang") && class.name == "Class"
        ));
        assert_eq!(inferred[0].confidence, Confidence::Certain);
    }

    #[test]
    fn test_infer_boolean_literal() {
        let root = parse_expression("true");
        fn find_literal(node: &SyntaxNode<JavaLanguage>) -> Option<SyntaxNode<JavaLanguage>> {
            if node.kind() == SyntaxKind::LITERAL_EXPR {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_literal(&child) {
                    return Some(found);
                }
            }
            None
        }

        let literal = find_literal(&root).expect("Should find literal");
        let inferred = infer_literal_type(&literal);

        assert!(matches!(
            inferred.ty,
            Type::Primitive(PrimitiveType::Boolean)
        ));
    }

    #[test]
    fn test_infer_new_expr_with_type_args() {
        // Test: new ArrayList<String>()
        let code = r"
            import java.util.ArrayList;
            class Test {
                void test() {
                    Object x = new ArrayList<String>();
                }
            }
        ";
        let tokens = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        // Find NEW_EXPR
        fn find_new_expr(node: &SyntaxNode<JavaLanguage>) -> Option<SyntaxNode<JavaLanguage>> {
            if node.kind() == SyntaxKind::NEW_EXPR {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_new_expr(&child) {
                    return Some(found);
                }
            }
            None
        }

        let new_expr = find_new_expr(&root).expect("Should find NEW_EXPR");

        // Create a mock workspace index (without actual files, type args still extracted)
        let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp")));
        let file_id = FileId::new(0);

        let inferred = infer_new_expr_type(&new_expr, file_id, &workspace_index);

        // Check that we got ArrayList with String type argument
        match &inferred.ty {
            Type::Class(c) => {
                assert!(
                    c.name.contains("ArrayList"),
                    "Expected ArrayList, got {}",
                    c.name
                );
                assert_eq!(c.type_arguments.len(), 1, "Expected 1 type argument");
                match &c.type_arguments[0] {
                    Type::Class(arg) => {
                        assert!(
                            arg.name.contains("String"),
                            "Expected String type arg, got {}",
                            arg.name
                        );
                    }
                    other => panic!("Expected Class type for type argument, got {other:?}"),
                }
            }
            other => panic!("Expected Class type, got {other:?}"),
        }
    }

    #[test]
    fn inferred_constructor_type_has_one_canonical_package() {
        let temp = tempfile::tempdir().expect("temp workspace");
        let index = Arc::new(WorkspaceIndex::new(temp.path().to_path_buf()));
        index
            .index_file_with_source(
                &temp.path().join("com/example/Pair.java"),
                "package com.example; record Pair<T>(T first, T second) {}",
            )
            .expect("record should index");
        let main_source = concat!(
            "package com.example; class Main { void test() { ",
            "Object value = new Pair<>(\"a\", \"b\"); } }"
        );
        let main_path = temp.path().join("com/example/Main.java");
        index
            .index_file_with_source(&main_path, main_source)
            .expect("caller should index");
        let file_id = index.get_file_id(&main_path).expect("caller file id");

        let tokens = Lexer::new(main_source).collect();
        let (green_node, errors) = Parser::parse(tokens);
        assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
        let root = SyntaxNode::new_root(green_node);
        let new_expr = root
            .descendants()
            .find(|node| node.kind() == SyntaxKind::NEW_EXPR)
            .expect("constructor expression");
        let inferred = infer_new_expr_type(&new_expr, file_id, &index);

        let Type::Class(class) = inferred.ty else {
            panic!("expected class type, got {:?}", inferred.ty);
        };
        assert_eq!(class.name, "Pair");
        assert_eq!(class.package.as_deref(), Some("com.example"));
        assert_eq!(class.qualified_name(), "com.example.Pair");
        assert!(class.is_diamond);
    }

    #[test]
    fn test_infer_nested_generics() {
        // Test: new HashMap<String, List<Integer>>()
        let code = r"
            import java.util.HashMap;
            import java.util.List;
            class Test {
                void test() {
                    Object x = new HashMap<String, List<Integer>>();
                }
            }
        ";
        let tokens = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root = SyntaxNode::new_root(green_node);

        fn find_new_expr(node: &SyntaxNode<JavaLanguage>) -> Option<SyntaxNode<JavaLanguage>> {
            if node.kind() == SyntaxKind::NEW_EXPR {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_new_expr(&child) {
                    return Some(found);
                }
            }
            None
        }

        let new_expr = find_new_expr(&root).expect("Should find NEW_EXPR");
        let workspace_index = Arc::new(WorkspaceIndex::new(std::path::PathBuf::from("/tmp")));
        let file_id = FileId::new(0);

        let inferred = infer_new_expr_type(&new_expr, file_id, &workspace_index);

        match &inferred.ty {
            Type::Class(c) => {
                assert!(
                    c.name.contains("HashMap"),
                    "Expected HashMap, got {}",
                    c.name
                );
                assert_eq!(c.type_arguments.len(), 2, "Expected 2 type arguments");

                // First type arg: String
                match &c.type_arguments[0] {
                    Type::Class(arg) => {
                        assert!(
                            arg.name.contains("String"),
                            "Expected String, got {}",
                            arg.name
                        );
                    }
                    other => panic!("Expected Class for first arg, got {other:?}"),
                }

                // Second type arg: List<Integer>
                match &c.type_arguments[1] {
                    Type::Class(list) => {
                        assert!(
                            list.name.contains("List"),
                            "Expected List, got {}",
                            list.name
                        );
                        assert_eq!(list.type_arguments.len(), 1, "Expected nested type arg");
                        match &list.type_arguments[0] {
                            Type::Class(inner) => {
                                assert!(
                                    inner.name.contains("Integer"),
                                    "Expected Integer, got {}",
                                    inner.name
                                );
                            }
                            other => panic!("Expected Class for nested arg, got {other:?}"),
                        }
                    }
                    other => panic!("Expected Class for second arg, got {other:?}"),
                }
            }
            other => panic!("Expected Class type, got {other:?}"),
        }
    }

    #[test]
    fn test_extract_class_name_from_qualified_type() {
        // Regression test: extract_class_name_from_type should return full qualified name
        // Bug: Previously returned only "ArrayList" instead of "java.util.ArrayList"
        let code = r"
            class Test {
                java.util.ArrayList<String> list;
            }
        ";
        let tokens = Lexer::new(code).collect();
        let (green_node, _errors) = Parser::parse(tokens);
        let root: SyntaxNode<JavaLanguage> = SyntaxNode::new_root(green_node);

        // Find CLASS_TYPE node
        fn find_class_type(node: &SyntaxNode<JavaLanguage>) -> Option<SyntaxNode<JavaLanguage>> {
            if node.kind() == SyntaxKind::CLASS_TYPE {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_class_type(&child) {
                    return Some(found);
                }
            }
            None
        }

        let class_type = find_class_type(&root).expect("Should find CLASS_TYPE");
        let extracted = extract_class_name_from_type(&class_type);

        assert_eq!(
            extracted,
            Some("java.util.ArrayList".to_string()),
            "Should extract full qualified name, not just simple name"
        );
    }
}
