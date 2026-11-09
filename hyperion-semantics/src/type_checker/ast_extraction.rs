// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::symbol::{ClassType, Type};
use hyperion_parser::{SyntaxKind, SyntaxNode};
use tracing::warn;

/// AST extraction helpers for TypeChecker
///
/// These methods extract type information and structure from syntax nodes.
impl super::TypeChecker {
    /// Extract variable name from a LOCAL_VAR_DECL node
    ///
    /// For `User user = new User();`, this should return `user` (the variable name),
    /// not `User` (the type name).
    ///
    /// Strategy: Skip all CLASS_TYPE/PRIMITIVE_TYPE/VAR_TYPE nodes, then return the first IDENT token found.
    /// This ensures we get the variable name, not a type name.
    #[allow(clippy::unused_self)]
    pub(super) fn extract_variable_name(&self, node: &SyntaxNode) -> Option<String> {
        // LOCAL_VAR_DECL structure:
        // - Explicit type: CLASS_TYPE/PRIMITIVE_TYPE + IDENT (variable name) + [= INITIALIZER]
        // - Var type: VAR_TYPE + IDENT (variable name) + [= INITIALIZER]
        let mut found_type = false;

        for elem in node.children_with_tokens() {
            match elem {
                // Mark that we've seen a type node (including VAR_TYPE)
                rowan::NodeOrToken::Node(child_node)
                    if matches!(
                        child_node.kind(),
                        SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::VAR_TYPE
                    ) =>
                {
                    found_type = true;
                }
                // Return first IDENT token after type node
                rowan::NodeOrToken::Token(token)
                    if token.kind() == SyntaxKind::IDENT && found_type =>
                {
                    return Some(token.text().to_string());
                }
                _ => {}
            }
        }

        // Defensive: Warn if no type node found - indicates AST structure changed
        if !found_type {
            warn!(
                "extract_variable_name: No type node found in LOCAL_VAR_DECL at {:?}",
                node.text_range()
            );
        }

        None
    }

    /// Extract the declared type from a LOCAL_VAR_DECL node
    ///
    /// Returns None for `var` declarations (type is inferred)
    pub(super) fn extract_declared_type(&self, node: &SyntaxNode) -> Option<Type> {
        use SyntaxKind::{CLASS_TYPE, PRIMITIVE_TYPE, VAR_KW};

        // Check if this is a `var` declaration
        if node
            .descendants_with_tokens()
            .any(|elem| elem.as_token().map(rowan::SyntaxToken::kind) == Some(VAR_KW))
        {
            return None; // var declarations are inferred
        }

        // Look for type nodes
        for child in node.children() {
            match child.kind() {
                PRIMITIVE_TYPE => {
                    return self
                        .extract_primitive_type(&child)
                        .map(|base| Self::wrap_with_array_dims(base, node));
                }
                CLASS_TYPE => {
                    let base = self.extract_contextual_class_type(&child);
                    return Some(Self::wrap_with_array_dims(base, node));
                }
                _ => continue,
            }
        }

        None
    }

    pub(super) fn extract_contextual_class_type(&self, node: &SyntaxNode) -> Type {
        self.syntax_type_resolver.as_ref().map_or_else(
            || Type::Class(Self::parse_class_type(node.text().to_string().trim())),
            |resolver| resolver.extract_class_type(node),
        )
    }

    pub(super) fn extract_param_name_and_type(&self, node: &SyntaxNode) -> Option<(String, Type)> {
        let mut param_type: Option<Type> = None;
        let mut param_name: Option<String> = None;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::PRIMITIVE_TYPE => {
                    param_type = self.extract_primitive_type(&child);
                }
                SyntaxKind::CLASS_TYPE => {
                    param_type = Some(self.extract_contextual_class_type(&child));
                }
                _ => {}
            }
        }

        // Direct children only. `descendants_with_tokens` descends into the type
        // node first, so `String s` yielded "String" and `java.util.List<T> items`
        // yielded "java" — every class-typed parameter was registered under its
        // type's name and the real parameter stayed untyped.
        for elem in node.children_with_tokens() {
            if let Some(token) = elem.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    param_name = Some(token.text().to_string());
                    break;
                }
            }
        }

        match (param_name, param_type) {
            (Some(name), Some(typ)) => Some((name, Self::wrap_with_array_dims(typ, node))),
            _ => None,
        }
    }

    pub(super) fn extract_field_name_and_type(&self, node: &SyntaxNode) -> Option<(String, Type)> {
        let mut field_type: Option<Type> = None;
        let mut found_type = false;

        for child in node.children() {
            match child.kind() {
                SyntaxKind::PRIMITIVE_TYPE => {
                    field_type = self.extract_primitive_type(&child);
                    found_type = true;
                }
                SyntaxKind::CLASS_TYPE => {
                    field_type = Some(self.extract_contextual_class_type(&child));
                    found_type = true;
                }
                _ => {}
            }
        }

        if !found_type {
            return None;
        }

        for elem in node.children_with_tokens() {
            if let Some(token) = elem.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    if let Some(typ) = field_type {
                        return Some((
                            token.text().to_string(),
                            Self::wrap_with_array_dims(typ, node),
                        ));
                    }
                }
            }
        }

        None
    }

    /// Wrap a base type in `Type::Array` per `[]` brackets present on the declaration.
    /// In the parser, each `[]` becomes a sibling `ARRAY_TYPE` child of the declaration node
    /// (LOCAL_VAR_DECL, PARAM, FIELD_DECL), e.g. `byte[][] x` → PRIMITIVE_TYPE + ARRAY_TYPE × 2.
    pub(super) fn wrap_with_array_dims(base: Type, node: &SyntaxNode) -> Type {
        let dims: u8 = node
            .children()
            .filter(|c| c.kind() == SyntaxKind::ARRAY_TYPE)
            .take(u8::MAX as usize)
            .count()
            .try_into()
            .unwrap_or(u8::MAX);
        if dims == 0 {
            base
        } else {
            Type::Array {
                element_type: Box::new(base),
                dimensions: dims,
            }
        }
    }

    #[allow(clippy::unused_self)]
    pub(super) fn extract_type_param_name(&self, node: &SyntaxNode) -> Option<String> {
        for elem in node.descendants_with_tokens() {
            if let Some(token) = elem.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    return Some(token.text().to_string());
                }
            }
        }
        None
    }

    /// Parse a fully qualified class name into a ClassType
    ///
    /// Handles generic type arguments: `"Map<String, List<Integer>>"` produces
    /// a ClassType with properly parsed type_arguments, not generics embedded in the name.
    ///
    /// Examples:
    /// - "String" → ClassType { name: "String", package: Some("java.lang") }
    /// - "java.lang.String" → ClassType { name: "String", package: Some("java.lang") }
    /// - "Map<String, Integer>" → ClassType { name: "Map", type_arguments: [String, Integer] }
    ///
    /// Automatically resolves well-known java.lang types like String, Integer, etc.
    pub(super) fn parse_class_type(fqn: &str) -> ClassType {
        let fqn = fqn.trim();

        let (base, generics_str) = split_base_and_generics(fqn);

        // Diamond operator: ArrayList<>
        if generics_str == Some("<>") {
            return build_class_type_from_base(base, vec![], true);
        }

        let type_arguments = generics_str
            .map(parse_generic_type_args)
            .unwrap_or_default();

        build_class_type_from_base(base, type_arguments, false)
    }

    /// Extract primitive type from a PRIMITIVE_TYPE node
    #[allow(clippy::unused_self)]
    pub(super) fn extract_primitive_type(&self, node: &SyntaxNode) -> Option<Type> {
        use crate::symbol::PrimitiveType::{Boolean, Byte, Char, Double, Float, Int, Long, Short};

        let text = node.text().to_string();
        let prim = match text.as_str() {
            "byte" => Byte,
            "short" => Short,
            "int" => Int,
            "long" => Long,
            "float" => Float,
            "double" => Double,
            "char" => Char,
            "boolean" => Boolean,
            _ => return None,
        };

        Some(Type::Primitive(prim))
    }

    /// Extract the type of the initializer expression
    pub(super) fn extract_initializer_type(&self, node: &SyntaxNode) -> Option<Type> {
        self.initializer_expression(node)
            .and_then(|expr| self.infer_expression_type(&expr))
    }

    /// The initializer expression node of a variable declaration.
    ///
    /// The initializer is the expression child after the type; in the AST
    /// structure that is CLASS_TYPE/PRIMITIVE_TYPE followed by
    /// LITERAL_EXPR/NEW_EXPR/etc.
    pub(super) fn initializer_expression(&self, node: &SyntaxNode) -> Option<SyntaxNode> {
        let mut last_expr: Option<SyntaxNode> = None;
        for child in node.children() {
            match child.kind() {
                // Skip type nodes
                SyntaxKind::CLASS_TYPE | SyntaxKind::PRIMITIVE_TYPE | SyntaxKind::ARRAY_TYPE => {
                    continue
                }
                // Expression nodes are the initializer
                SyntaxKind::LITERAL_EXPR
                | SyntaxKind::NEW_EXPR
                | SyntaxKind::BINARY_EXPR
                | SyntaxKind::UNARY_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::METHOD_CALL_EXPR
                | SyntaxKind::PAREN_EXPR
                | SyntaxKind::TERNARY_EXPR
                | SyntaxKind::CAST_EXPR
                | SyntaxKind::NAME_EXPR
                | SyntaxKind::THIS_EXPR
                | SyntaxKind::SUPER_EXPR => {
                    last_expr = Some(child);
                }
                _ => continue,
            }
        }
        last_expr
    }

    /// Infer type from a literal token kind
    ///
    /// Extracts the type from literal token kinds (INT_LITERAL, STRING_LITERAL, etc.)
    /// This helper avoids duplicating literal type inference logic.
    ///
    /// Used by infer_expression_type for both direct literal nodes and LITERAL_EXPR wrappers.
    pub(super) fn infer_literal_type(kind: SyntaxKind) -> Option<Type> {
        use crate::symbol::PrimitiveType::{Boolean, Double, Int, Long};
        use SyntaxKind::{
            BOOLEAN_LITERAL, DOUBLE_LITERAL, FLOAT_LITERAL, INT_LITERAL, LONG_LITERAL,
            NULL_LITERAL, STRING_LITERAL, TEXT_BLOCK_LITERAL,
        };

        match kind {
            INT_LITERAL => Some(Type::Primitive(Int)),
            LONG_LITERAL => Some(Type::Primitive(Long)),
            STRING_LITERAL | TEXT_BLOCK_LITERAL => {
                Some(Type::Class(Self::parse_class_type("java.lang.String")))
            }
            NULL_LITERAL => Some(Type::Null),
            BOOLEAN_LITERAL => Some(Type::Primitive(Boolean)),
            FLOAT_LITERAL | DOUBLE_LITERAL => Some(Type::Primitive(Double)),
            _ => None,
        }
    }

    /// Expression type inference with FileSemantics fast path
    ///
    /// The semantic walker pre-computes types for all expression nodes (METHOD_CALL_EXPR,
    /// FIELD_ACCESS_EXPR, NEW_EXPR, CAST_EXPR, BINARY_EXPR, UNARY_EXPR, TERNARY_EXPR,
    /// LITERAL_EXPR, PAREN_EXPR). This method checks the cache first, then falls back
    /// to literal token inference for bare tokens that aren't wrapped in expression nodes.
    pub(super) fn infer_expression_type(&self, node: &SyntaxNode) -> Option<Type> {
        // Fast path: check pre-computed expression types from FileSemantics
        if let Some(ref file_sem) = self.file_semantics {
            if let Some(cached) = file_sem.expression_type_at(node.text_range()) {
                return Some(cached.clone());
            }
        }

        // For direct literal tokens (not wrapped in LITERAL_EXPR), infer from token kind
        if let Some(ty) = Self::infer_literal_type(node.kind()) {
            return Some(ty);
        }

        // Explicit expression types use the same lexical/import context as
        // declarations, even when FileSemantics was not supplied by the caller.
        if matches!(node.kind(), SyntaxKind::NEW_EXPR | SyntaxKind::CAST_EXPR) {
            if let Some(resolver) = &self.syntax_type_resolver {
                let inferred = resolver.infer_expression_type(node);
                return (!matches!(inferred, Type::Unknown)).then_some(inferred);
            }
        }
        if node.kind() == SyntaxKind::NEW_EXPR {
            if let Some(class_node) = node.children().find(|c| c.kind() == SyntaxKind::CLASS_TYPE) {
                return Some(Type::Class(Self::parse_class_type(
                    &class_node.text().to_string(),
                )));
            }
        }

        // For LITERAL_EXPR wrappers not in cache, check inner tokens
        if node.kind() == SyntaxKind::LITERAL_EXPR {
            for elem in node.children_with_tokens() {
                if let rowan::NodeOrToken::Token(token) = elem {
                    if let Some(ty) = Self::infer_literal_type(token.kind()) {
                        return Some(ty);
                    }
                }
            }
        }

        None
    }

    /// Extract the type of the value being assigned in an ASSIGNMENT node
    pub(super) fn extract_assignment_value_type(&self, node: &SyntaxNode) -> Option<Type> {
        self.assignment_value_expression(node)
            .and_then(|expr| self.infer_expression_type(&expr))
    }

    /// The right-hand side of an ASSIGNMENT_EXPR node.
    ///
    /// ASSIGNMENT_EXPR structure:
    ///   Sub-token: EQ = "="
    ///   Sub-token: WHITESPACE = " "
    ///   Sub-child: LITERAL_EXPR = "\"Hey\""
    /// The value is the last expression child node.
    pub(super) fn assignment_value_expression(&self, node: &SyntaxNode) -> Option<SyntaxNode> {
        let mut last_expr: Option<SyntaxNode> = None;
        for child in node.children() {
            match child.kind() {
                SyntaxKind::LITERAL_EXPR
                | SyntaxKind::NEW_EXPR
                | SyntaxKind::BINARY_EXPR
                | SyntaxKind::UNARY_EXPR
                | SyntaxKind::FIELD_ACCESS_EXPR
                | SyntaxKind::METHOD_CALL_EXPR
                | SyntaxKind::PAREN_EXPR
                | SyntaxKind::TERNARY_EXPR
                | SyntaxKind::CAST_EXPR
                | SyntaxKind::NAME_EXPR
                | SyntaxKind::THIS_EXPR
                | SyntaxKind::SUPER_EXPR => {
                    last_expr = Some(child);
                }
                _ => continue,
            }
        }
        last_expr
    }

    /// Extract the identifier chain from an assignment target.
    ///
    /// For `entry.id = ...`, returns ["entry", "id"].
    /// For `entry.inner.id = ...`, returns ["entry", "inner", "id"].
    pub(super) fn extract_assignment_target_chain(
        &self,
        assignment_node: &SyntaxNode,
    ) -> Vec<String> {
        let Some(parent) = assignment_node.parent() else {
            return Vec::new();
        };

        let mut idents = Vec::new();
        for child in parent.children() {
            if child == *assignment_node {
                break;
            }

            if matches!(
                child.kind(),
                SyntaxKind::NAME_EXPR | SyntaxKind::FIELD_ACCESS_EXPR | SyntaxKind::THIS_EXPR
            ) {
                let chain = self.collect_ident_chain(&child);
                if !chain.is_empty() {
                    idents = chain;
                }
            }
        }

        idents
    }

    #[allow(clippy::unused_self)]
    fn collect_ident_chain(&self, node: &SyntaxNode) -> Vec<String> {
        node.descendants_with_tokens()
            .filter_map(|elem| match elem {
                rowan::NodeOrToken::Token(token) if token.kind() == SyntaxKind::IDENT => {
                    Some(token.text().to_string())
                }
                _ => None,
            })
            .collect()
    }
}

// Well-known java.lang types that don't need fully qualified names
const JAVA_LANG_TYPES: &[&str] = &[
    // Primitive wrappers
    "String",
    "Integer",
    "Long",
    "Short",
    "Byte",
    "Float",
    "Double",
    "Boolean",
    "Character",
    "Void",
    // Core classes
    "Object",
    "Class",
    "System",
    "Math",
    "Number",
    // String builders
    "StringBuilder",
    "StringBuffer",
    // Exceptions and errors
    "Throwable",
    "Exception",
    "Error",
    "RuntimeException",
    "IllegalArgumentException",
    "IllegalStateException",
    "NullPointerException",
    "IndexOutOfBoundsException",
    "UnsupportedOperationException",
    // Threads
    "Thread",
    "Runnable",
    "ThreadLocal",
    // Interfaces
    "Comparable",
    "Iterable",
    "Cloneable",
    "AutoCloseable",
    // Process/Runtime
    "Process",
    "ProcessBuilder",
    "Runtime",
    // Annotations
    "Override",
    "Deprecated",
    "SuppressWarnings",
    "FunctionalInterface",
    // Misc
    "Enum",
    "Package",
    "StackTraceElement",
];

/// Split a type string into (base_name, optional_generics_str).
///
/// `"java.util.Map<String, Foo>"` → `("java.util.Map", Some("<String, Foo>"))`
/// `"String"` → `("String", None)`
fn split_base_and_generics(fqn: &str) -> (&str, Option<&str>) {
    fqn.find('<')
        .map_or((fqn, None), |lt_pos| (&fqn[..lt_pos], Some(&fqn[lt_pos..])))
}

/// Parse the content between `<` and matching `>` into Type arguments.
///
/// Input must include the outer angle brackets, e.g. `"<String, List<Integer>>"`.
fn parse_generic_type_args(generics: &str) -> Vec<Type> {
    // Strip outer < and >
    let inner = generics
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(generics);

    if inner.is_empty() {
        return vec![];
    }

    // Split by commas at depth 0 (respecting nested < >)
    let mut args = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;

    for (i, ch) in inner.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                args.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    args.push(inner[start..].trim());

    args.into_iter()
        .filter(|s| !s.is_empty())
        .map(parse_single_type_arg)
        .collect()
}

/// Parse a single generic type argument string into a Type.
fn parse_single_type_arg(arg: &str) -> Type {
    let arg = arg.trim();

    // Wildcard: "?", "? extends Foo", "? super Bar"
    if arg == "?" {
        return Type::Wildcard {
            upper_bound: None,
            lower_bound: None,
        };
    }
    if let Some(bound) = arg.strip_prefix("? extends ") {
        let bound_type = parse_type_arg_as_type(bound.trim());
        return Type::Wildcard {
            upper_bound: Some(Box::new(bound_type)),
            lower_bound: None,
        };
    }
    if let Some(bound) = arg.strip_prefix("? super ") {
        let bound_type = parse_type_arg_as_type(bound.trim());
        return Type::Wildcard {
            upper_bound: None,
            lower_bound: Some(Box::new(bound_type)),
        };
    }

    parse_type_arg_as_type(arg)
}

/// Parse a type argument string (not a wildcard) into a Type.
fn parse_type_arg_as_type(arg: &str) -> Type {
    let (base, generics_str) = split_base_and_generics(arg);

    let type_arguments = generics_str
        .map(parse_generic_type_args)
        .unwrap_or_default();

    Type::Class(build_class_type_from_base(base, type_arguments, false))
}

/// Build a ClassType from a base name (no generics), resolving java.lang types.
fn build_class_type_from_base(
    base: &str,
    type_arguments: Vec<Type>,
    is_diamond: bool,
) -> ClassType {
    let (name, package) = base.rfind('.').map_or_else(
        || {
            if JAVA_LANG_TYPES.contains(&base) {
                (base.to_string(), Some("java.lang".into()))
            } else {
                (base.to_string(), None)
            }
        },
        |last_dot| {
            (
                base[last_dot + 1..].to_string(),
                Some(base[..last_dot].to_string()),
            )
        },
    );

    if is_diamond {
        ClassType::with_diamond(name, package)
    } else if type_arguments.is_empty() {
        ClassType::new(name, package)
    } else {
        ClassType::with_type_args(name, package, type_arguments)
    }
}
