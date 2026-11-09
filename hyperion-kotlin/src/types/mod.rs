// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Kotlin Type System
//!
//! This module provides a comprehensive representation of Kotlin types,
//! including support for generics, variance, and type inference.

use std::collections::HashMap;
use std::fmt;

/// Variance annotation for type parameters
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Variance {
    /// Invariant: T (exact match required)
    #[default]
    Invariant,
    /// Covariant: out T (subtype allowed)
    Covariant,
    /// Contravariant: in T (supertype allowed)
    Contravariant,
}

/// Nullability of a type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Nullability {
    /// Non-null type (e.g., String)
    #[default]
    NonNull,
    /// Nullable type (e.g., String?)
    Nullable,
    /// Platform type from Java interop (e.g., String!)
    Platform,
}

/// A type argument in a generic type (e.g., the String in List<String>)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeArgument {
    /// Concrete type argument: List<String>
    Type(KotlinType),
    /// Star projection: List<*>
    Star,
    /// Projected type argument: List<out Number>, List<in String>
    Projected {
        variance: Variance,
        bound: KotlinType,
    },
}

impl TypeArgument {
    /// Create a concrete type argument
    pub const fn concrete(ty: KotlinType) -> Self {
        TypeArgument::Type(ty)
    }

    /// Create a star projection
    pub const fn star() -> Self {
        TypeArgument::Star
    }

    /// Create a covariant projection (out T)
    pub const fn out_of(ty: KotlinType) -> Self {
        TypeArgument::Projected {
            variance: Variance::Covariant,
            bound: ty,
        }
    }

    /// Create a contravariant projection (in T)
    pub const fn in_of(ty: KotlinType) -> Self {
        TypeArgument::Projected {
            variance: Variance::Contravariant,
            bound: ty,
        }
    }
}

/// A type parameter declaration (e.g., T in class Box<T>)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParameter {
    /// Name of the type parameter (e.g., "T", "K", "V")
    pub name: String,
    /// Declaration-site variance
    pub variance: Variance,
    /// Upper bounds (T : Comparable<T>)
    pub bounds: Vec<KotlinType>,
    /// Whether this is a reified type parameter
    pub reified: bool,
}

impl TypeParameter {
    /// Create a simple type parameter with no bounds
    pub fn simple(name: impl Into<String>) -> Self {
        TypeParameter {
            name: name.into(),
            variance: Variance::Invariant,
            bounds: vec![],
            reified: false,
        }
    }

    /// Create a covariant type parameter (out T)
    pub fn covariant(name: impl Into<String>) -> Self {
        TypeParameter {
            name: name.into(),
            variance: Variance::Covariant,
            bounds: vec![],
            reified: false,
        }
    }

    /// Create a contravariant type parameter (in T)
    pub fn contravariant(name: impl Into<String>) -> Self {
        TypeParameter {
            name: name.into(),
            variance: Variance::Contravariant,
            bounds: vec![],
            reified: false,
        }
    }
}

/// Comprehensive Kotlin type representation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KotlinType {
    /// Class or interface type: String, List<Int>, Map<String, User>
    Class {
        /// Fully qualified name (e.g., "kotlin.String", "kotlin.collections.List")
        fqn: String,
        /// Type arguments for generic types
        type_arguments: Vec<TypeArgument>,
        /// Nullability
        nullability: Nullability,
    },

    /// Type variable: T, K, V (used in generic contexts)
    TypeVariable {
        /// Name of the type variable
        name: String,
        /// Upper bounds
        bounds: Vec<KotlinType>,
    },

    /// Function type: (Int, String) -> Boolean, String.() -> Unit
    Function {
        /// Receiver type for extension function types
        receiver: Option<Box<KotlinType>>,
        /// Parameter types
        parameters: Vec<KotlinType>,
        /// Return type
        return_type: Box<KotlinType>,
        /// Nullability of the function type itself
        nullability: Nullability,
    },

    /// Star projection placeholder (used during resolution)
    Star,

    /// Nothing type (subtype of all types)
    Nothing,

    /// Unit type (like void)
    Unit,

    /// Any type (supertype of all non-null types)
    Any,

    /// Unknown/error type
    Unknown,
}

impl KotlinType {
    // ========================================================================
    // Constructors
    // ========================================================================

    /// Create a simple non-null class type
    pub fn simple(name: impl Into<String>) -> Self {
        KotlinType::Class {
            fqn: name.into(),
            type_arguments: vec![],
            nullability: Nullability::NonNull,
        }
    }

    /// Create a generic class type
    pub fn generic(name: impl Into<String>, args: Vec<KotlinType>) -> Self {
        KotlinType::Class {
            fqn: name.into(),
            type_arguments: args.into_iter().map(TypeArgument::Type).collect(),
            nullability: Nullability::NonNull,
        }
    }

    /// Create a generic class type with type arguments
    pub fn generic_with_args(name: impl Into<String>, args: Vec<TypeArgument>) -> Self {
        KotlinType::Class {
            fqn: name.into(),
            type_arguments: args,
            nullability: Nullability::NonNull,
        }
    }

    /// Create a nullable version of this type
    pub fn nullable(self) -> Self {
        match self {
            KotlinType::Class {
                fqn,
                type_arguments,
                ..
            } => KotlinType::Class {
                fqn,
                type_arguments,
                nullability: Nullability::Nullable,
            },
            KotlinType::Function {
                receiver,
                parameters,
                return_type,
                ..
            } => KotlinType::Function {
                receiver,
                parameters,
                return_type,
                nullability: Nullability::Nullable,
            },
            // The type of `null` is `Nothing?`. `Nothing` has no nullable variant of its own.
            KotlinType::Nothing => KotlinType::Class {
                fqn: "Nothing".to_string(),
                type_arguments: Vec::new(),
                nullability: Nullability::Nullable,
            },
            other => other,
        }
    }

    /// Create a type variable
    pub fn type_var(name: impl Into<String>) -> Self {
        KotlinType::TypeVariable {
            name: name.into(),
            bounds: vec![],
        }
    }

    /// Create a function type
    pub fn function(params: Vec<KotlinType>, ret: KotlinType) -> Self {
        KotlinType::Function {
            receiver: None,
            parameters: params,
            return_type: Box::new(ret),
            nullability: Nullability::NonNull,
        }
    }

    // ========================================================================
    // Common Types
    // ========================================================================

    pub fn int() -> Self {
        KotlinType::simple("kotlin.Int")
    }
    pub fn long() -> Self {
        KotlinType::simple("kotlin.Long")
    }
    pub fn float() -> Self {
        KotlinType::simple("kotlin.Float")
    }
    pub fn double() -> Self {
        KotlinType::simple("kotlin.Double")
    }
    pub fn boolean() -> Self {
        KotlinType::simple("kotlin.Boolean")
    }
    pub fn char() -> Self {
        KotlinType::simple("kotlin.Char")
    }
    pub fn string() -> Self {
        KotlinType::simple("kotlin.String")
    }
    pub const fn unit() -> Self {
        KotlinType::Unit
    }
    pub const fn any() -> Self {
        KotlinType::Any
    }
    pub const fn nothing() -> Self {
        KotlinType::Nothing
    }

    /// Create List<T>
    pub fn list(element: KotlinType) -> Self {
        KotlinType::generic("kotlin.collections.List", vec![element])
    }

    /// Create MutableList<T>
    pub fn mutable_list(element: KotlinType) -> Self {
        KotlinType::generic("kotlin.collections.MutableList", vec![element])
    }

    /// Create Set<T>
    pub fn set(element: KotlinType) -> Self {
        KotlinType::generic("kotlin.collections.Set", vec![element])
    }

    /// Create Map<K, V>
    pub fn map(key: KotlinType, value: KotlinType) -> Self {
        KotlinType::generic("kotlin.collections.Map", vec![key, value])
    }

    // ========================================================================
    // Type Operations
    // ========================================================================

    /// Check if this type is nullable
    pub fn is_nullable(&self) -> bool {
        match self {
            KotlinType::Class { nullability, .. } | KotlinType::Function { nullability, .. } => {
                *nullability == Nullability::Nullable
            }
            KotlinType::Nothing => true, // Nothing? is valid
            _ => false,
        }
    }

    /// Get the simple name (without package)
    pub fn simple_name(&self) -> &str {
        match self {
            KotlinType::Class { fqn, .. } => fqn.rsplit('.').next().unwrap_or(fqn),
            KotlinType::TypeVariable { name, .. } => name,
            KotlinType::Function { .. } => "Function",
            KotlinType::Star => "*",
            KotlinType::Nothing => "Nothing",
            KotlinType::Unit => "Unit",
            KotlinType::Any => "Any",
            KotlinType::Unknown => "?",
        }
    }

    /// Format the type as a Kotlin type string
    ///
    /// This is an alias for `to_string()` for API parity with the legacy Type enum.
    pub fn format(&self) -> String {
        self.to_string()
    }

    /// Apply type substitutions (replace type variables with concrete types)
    pub fn substitute(&self, substitutions: &HashMap<String, KotlinType>) -> KotlinType {
        match self {
            KotlinType::TypeVariable { name, .. } => substitutions
                .get(name)
                .cloned()
                .unwrap_or_else(|| self.clone()),

            KotlinType::Class {
                fqn,
                type_arguments,
                nullability,
            } => KotlinType::Class {
                fqn: fqn.clone(),
                type_arguments: type_arguments
                    .iter()
                    .map(|arg| match arg {
                        TypeArgument::Type(ty) => TypeArgument::Type(ty.substitute(substitutions)),
                        TypeArgument::Star => TypeArgument::Star,
                        TypeArgument::Projected { variance, bound } => TypeArgument::Projected {
                            variance: *variance,
                            bound: bound.substitute(substitutions),
                        },
                    })
                    .collect(),
                nullability: *nullability,
            },

            KotlinType::Function {
                receiver,
                parameters,
                return_type,
                nullability,
            } => KotlinType::Function {
                receiver: receiver
                    .as_ref()
                    .map(|r| Box::new(r.substitute(substitutions))),
                parameters: parameters
                    .iter()
                    .map(|p| p.substitute(substitutions))
                    .collect(),
                return_type: Box::new(return_type.substitute(substitutions)),
                nullability: *nullability,
            },

            _ => self.clone(),
        }
    }
}

impl fmt::Display for KotlinType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KotlinType::Class {
                fqn,
                type_arguments,
                nullability,
            } => {
                // Use simple name for display
                let name = fqn.rsplit('.').next().unwrap_or(fqn);
                write!(f, "{name}")?;
                if !type_arguments.is_empty() {
                    write!(f, "<")?;
                    for (i, arg) in type_arguments.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    write!(f, ">")?;
                }
                if *nullability == Nullability::Nullable {
                    write!(f, "?")?;
                }
                Ok(())
            }
            KotlinType::TypeVariable { name, .. } => write!(f, "{name}"),
            KotlinType::Function {
                receiver,
                parameters,
                return_type,
                nullability,
            } => {
                if let Some(recv) = receiver {
                    write!(f, "{recv}.")?;
                }
                write!(f, "(")?;
                for (i, param) in parameters.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{param}")?;
                }
                write!(f, ") -> {return_type}")?;
                if *nullability == Nullability::Nullable {
                    write!(f, "?")?;
                }
                Ok(())
            }
            KotlinType::Star => write!(f, "*"),
            KotlinType::Nothing => write!(f, "Nothing"),
            KotlinType::Unit => write!(f, "Unit"),
            KotlinType::Any => write!(f, "Any"),
            KotlinType::Unknown => write!(f, "?"),
        }
    }
}

impl fmt::Display for TypeArgument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeArgument::Type(ty) => write!(f, "{ty}"),
            TypeArgument::Star => write!(f, "*"),
            TypeArgument::Projected { variance, bound } => {
                let var_str = match variance {
                    Variance::Covariant => "out ",
                    Variance::Contravariant => "in ",
                    Variance::Invariant => "",
                };
                write!(f, "{var_str}{bound}")
            }
        }
    }
}

impl fmt::Display for Variance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Variance::Invariant => write!(f, ""),
            Variance::Covariant => write!(f, "out"),
            Variance::Contravariant => write!(f, "in"),
        }
    }
}

// ============================================================================
// Conversion from old Type representation
// ============================================================================

impl From<crate::type_extractor::Type> for KotlinType {
    fn from(ty: crate::type_extractor::Type) -> Self {
        use crate::type_extractor::Type;

        match ty {
            Type::Simple(name) => {
                // Map common names to proper FQNs
                let fqn = match name.as_str() {
                    "Int" => "kotlin.Int",
                    "Long" => "kotlin.Long",
                    "Float" => "kotlin.Float",
                    "Double" => "kotlin.Double",
                    "Boolean" => "kotlin.Boolean",
                    "Char" => "kotlin.Char",
                    "String" => "kotlin.String",
                    "Unit" => return KotlinType::Unit,
                    "Nothing" => return KotlinType::Nothing,
                    "Any" => return KotlinType::Any,
                    _ => &name,
                };
                KotlinType::simple(fqn)
            }
            Type::Generic(name, args) => {
                let fqn = match name.as_str() {
                    "List" => "kotlin.collections.List",
                    "MutableList" => "kotlin.collections.MutableList",
                    "ArrayList" => "kotlin.collections.ArrayList",
                    "Set" => "kotlin.collections.Set",
                    "MutableSet" => "kotlin.collections.MutableSet",
                    "HashSet" => "kotlin.collections.HashSet",
                    "Map" => "kotlin.collections.Map",
                    "MutableMap" => "kotlin.collections.MutableMap",
                    "HashMap" => "kotlin.collections.HashMap",
                    "Array" => "kotlin.Array",
                    "Optional" => "java.util.Optional",
                    _ => &name,
                };
                let type_args: Vec<KotlinType> = args.into_iter().map(KotlinType::from).collect();
                KotlinType::generic(fqn, type_args)
            }
            Type::Nullable(inner) => KotlinType::from(*inner).nullable(),
            Type::Function(params, ret) => {
                let param_types: Vec<KotlinType> =
                    params.into_iter().map(KotlinType::from).collect();
                let return_type = KotlinType::from(*ret);
                KotlinType::function(param_types, return_type)
            }
            Type::Unknown => KotlinType::Unknown,
        }
    }
}

impl KotlinType {
    /// Convert back to the simpler Type representation (for compatibility)
    pub fn to_simple_type(&self) -> crate::type_extractor::Type {
        use crate::type_extractor::Type;

        match self {
            KotlinType::Class {
                fqn,
                type_arguments,
                nullability,
            } => {
                // Use simple name for backward compatibility
                let simple_name = fqn.rsplit('.').next().unwrap_or(fqn).to_string();
                let base = if type_arguments.is_empty() {
                    Type::Simple(simple_name)
                } else {
                    let args: Vec<Type> = type_arguments
                        .iter()
                        .map(|arg| match arg {
                            TypeArgument::Type(ty) => ty.to_simple_type(),
                            TypeArgument::Star => Type::Unknown,
                            TypeArgument::Projected { bound, .. } => bound.to_simple_type(),
                        })
                        .collect();
                    Type::Generic(simple_name, args)
                };
                if *nullability == Nullability::Nullable {
                    Type::Nullable(Box::new(base))
                } else {
                    base
                }
            }
            KotlinType::TypeVariable { name, .. } => Type::Simple(name.clone()),
            KotlinType::Function {
                parameters,
                return_type,
                nullability,
                ..
            } => {
                let params: Vec<Type> = parameters.iter().map(KotlinType::to_simple_type).collect();
                let ret = return_type.to_simple_type();
                let base = Type::Function(params, Box::new(ret));
                if *nullability == Nullability::Nullable {
                    Type::Nullable(Box::new(base))
                } else {
                    base
                }
            }
            KotlinType::Star | KotlinType::Unknown => Type::Unknown,
            KotlinType::Nothing => Type::Simple("Nothing".to_string()),
            KotlinType::Unit => Type::Simple("Unit".to_string()),
            KotlinType::Any => Type::Simple("Any".to_string()),
        }
    }
}

// ============================================================================
// Type Parsing from AST
// ============================================================================

use crate::{SyntaxKind, SyntaxNode};

impl KotlinType {
    /// Parse a KotlinType from an AST node (TYPE_REF or NULLABLE_TYPE)
    pub fn from_ast(node: &SyntaxNode) -> Self {
        match node.kind() {
            SyntaxKind::NULLABLE_TYPE => Self::parse_nullable_type(node),
            SyntaxKind::TYPE_REF => Self::parse_type_ref(node),
            _ => {
                // Try to parse from children
                for child in node.children() {
                    let ty = Self::from_ast(&child);
                    if ty != KotlinType::Unknown {
                        return ty;
                    }
                }
                KotlinType::Unknown
            }
        }
    }

    /// Parse a nullable type node
    fn parse_nullable_type(node: &SyntaxNode) -> Self {
        // Find base type
        for child in node.children() {
            if child.kind() == SyntaxKind::TYPE_REF {
                return Self::parse_type_ref(&child).nullable();
            }
        }
        // Fallback: parse text
        let text = node.text().to_string();
        let base = text.trim_end_matches('?').trim();
        KotlinType::simple(base).nullable()
    }

    /// Parse a type reference node
    fn parse_type_ref(node: &SyntaxNode) -> Self {
        let mut name: Option<String> = None;
        let mut type_args: Vec<TypeArgument> = Vec::new();
        let mut is_nullable = false;

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::IDENT if name.is_none() => {
                        name = Some(token.text().to_string());
                    }
                    SyntaxKind::QUESTION => {
                        is_nullable = true;
                    }
                    _ => {}
                },
                rowan::NodeOrToken::Node(n) => {
                    if n.kind() == SyntaxKind::TYPE_ARGUMENT_LIST {
                        type_args = Self::parse_type_arguments(&n);
                    }
                }
            }
        }

        let base_type = name.map_or(KotlinType::Unknown, |n| {
            let fqn = Self::resolve_simple_name_to_fqn(&n);
            if type_args.is_empty() {
                KotlinType::simple(fqn)
            } else {
                KotlinType::generic_with_args(fqn, type_args)
            }
        });

        if is_nullable {
            base_type.nullable()
        } else {
            base_type
        }
    }

    /// Parse type arguments from TYPE_ARGUMENT_LIST
    ///
    /// Handles:
    /// - Simple types: List<String>
    /// - Star projection: List<*>
    /// - Variance projections: List<out T>, List<in T>
    fn parse_type_arguments(node: &SyntaxNode) -> Vec<TypeArgument> {
        let mut args = Vec::new();
        let mut pending_variance: Option<Variance> = None;

        for child in node.children_with_tokens() {
            match child {
                rowan::NodeOrToken::Token(token) => match token.kind() {
                    SyntaxKind::STAR => {
                        args.push(TypeArgument::Star);
                        pending_variance = None;
                    }
                    SyntaxKind::OUT_KW => {
                        pending_variance = Some(Variance::Covariant);
                    }
                    SyntaxKind::IN_KW => {
                        pending_variance = Some(Variance::Contravariant);
                    }
                    _ => {}
                },
                rowan::NodeOrToken::Node(n) => {
                    if matches!(n.kind(), SyntaxKind::TYPE_REF | SyntaxKind::NULLABLE_TYPE) {
                        let parsed_type = Self::from_ast(&n);
                        let arg = match pending_variance.take() {
                            Some(v) => TypeArgument::Projected {
                                variance: v,
                                bound: parsed_type,
                            },
                            None => TypeArgument::Type(parsed_type),
                        };
                        args.push(arg);
                    }
                }
            }
        }

        args
    }

    /// Resolve simple name to FQN for well-known types
    fn resolve_simple_name_to_fqn(name: &str) -> String {
        match name {
            // Primitives
            "Int" => "kotlin.Int".to_string(),
            "Long" => "kotlin.Long".to_string(),
            "Short" => "kotlin.Short".to_string(),
            "Byte" => "kotlin.Byte".to_string(),
            "Float" => "kotlin.Float".to_string(),
            "Double" => "kotlin.Double".to_string(),
            "Boolean" => "kotlin.Boolean".to_string(),
            "Char" => "kotlin.Char".to_string(),
            "String" => "kotlin.String".to_string(),
            // Special types
            "Unit" => "kotlin.Unit".to_string(),
            "Nothing" => "kotlin.Nothing".to_string(),
            "Any" => "kotlin.Any".to_string(),
            // Collections
            "List" => "kotlin.collections.List".to_string(),
            "MutableList" => "kotlin.collections.MutableList".to_string(),
            "ArrayList" => "kotlin.collections.ArrayList".to_string(),
            "Set" => "kotlin.collections.Set".to_string(),
            "MutableSet" => "kotlin.collections.MutableSet".to_string(),
            "HashSet" => "kotlin.collections.HashSet".to_string(),
            "Map" => "kotlin.collections.Map".to_string(),
            "MutableMap" => "kotlin.collections.MutableMap".to_string(),
            "HashMap" => "kotlin.collections.HashMap".to_string(),
            "Array" => "kotlin.Array".to_string(),
            "Sequence" => "kotlin.sequences.Sequence".to_string(),
            "Iterable" => "kotlin.collections.Iterable".to_string(),
            "Collection" => "kotlin.collections.Collection".to_string(),
            // Java interop
            "Optional" => "java.util.Optional".to_string(),
            // Default: keep as-is (user type or not yet mapped)
            _ => name.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_type_display() {
        assert_eq!(KotlinType::string().to_string(), "String");
        assert_eq!(KotlinType::int().to_string(), "Int");
    }

    #[test]
    fn test_generic_type_display() {
        let list_string = KotlinType::list(KotlinType::string());
        assert_eq!(list_string.to_string(), "List<String>");

        let map = KotlinType::map(KotlinType::string(), KotlinType::int());
        assert_eq!(map.to_string(), "Map<String, Int>");
    }

    #[test]
    fn test_nullable_type_display() {
        let nullable_string = KotlinType::string().nullable();
        assert_eq!(nullable_string.to_string(), "String?");
    }

    #[test]
    fn test_function_type_display() {
        let fn_type = KotlinType::function(vec![KotlinType::int()], KotlinType::string());
        assert_eq!(fn_type.to_string(), "(Int) -> String");
    }

    #[test]
    fn test_type_substitution() {
        let type_var = KotlinType::type_var("T");
        let list_t = KotlinType::generic("List", vec![type_var]);

        let mut subs = HashMap::new();
        subs.insert("T".to_string(), KotlinType::string());

        let result = list_t.substitute(&subs);
        assert_eq!(result.to_string(), "List<String>");
    }

    #[test]
    fn test_star_projection() {
        let list_star = KotlinType::generic_with_args("List", vec![TypeArgument::Star]);
        assert_eq!(list_star.to_string(), "List<*>");
    }

    #[test]
    fn test_projected_type() {
        let list_out = KotlinType::generic_with_args(
            "List",
            vec![TypeArgument::out_of(KotlinType::simple("Number"))],
        );
        assert_eq!(list_out.to_string(), "List<out Number>");
    }

    // ===== Type Conversion Tests =====

    #[test]
    fn test_conversion_from_old_type_simple() {
        use crate::type_extractor::Type;

        let old_type = Type::Simple("String".to_string());
        let kotlin_type: KotlinType = old_type.into();

        assert_eq!(kotlin_type.to_string(), "String");
        match kotlin_type {
            KotlinType::Class { fqn, .. } => assert_eq!(fqn, "kotlin.String"),
            _ => panic!("Expected Class type"),
        }
    }

    #[test]
    fn test_conversion_from_old_type_generic() {
        use crate::type_extractor::Type;

        let old_type = Type::Generic("List".to_string(), vec![Type::Simple("String".to_string())]);
        let kotlin_type: KotlinType = old_type.into();

        assert_eq!(kotlin_type.to_string(), "List<String>");
    }

    #[test]
    fn test_conversion_from_old_type_nullable() {
        use crate::type_extractor::Type;

        let old_type = Type::Nullable(Box::new(Type::Simple("String".to_string())));
        let kotlin_type: KotlinType = old_type.into();

        assert_eq!(kotlin_type.to_string(), "String?");
    }

    #[test]
    fn test_conversion_roundtrip() {
        use crate::type_extractor::Type;

        // Test that we can convert to KotlinType and back
        let old_type = Type::Generic(
            "Map".to_string(),
            vec![
                Type::Simple("String".to_string()),
                Type::Simple("Int".to_string()),
            ],
        );
        let kotlin_type: KotlinType = old_type.clone().into();
        let back = kotlin_type.to_simple_type();

        assert_eq!(back, old_type);
    }

    // ===== AST Parsing Tests =====

    #[test]
    fn test_parse_simple_type_from_ast() {
        let source = "val x: String = \"\"";
        let parse_tree = crate::parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_type_ref(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::TYPE_REF {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_type_ref(&child) {
                    return Some(found);
                }
            }
            None
        }

        let type_ref = find_type_ref(&root).expect("TYPE_REF not found");
        let kotlin_type = KotlinType::from_ast(&type_ref);

        assert_eq!(kotlin_type.to_string(), "String");
        match kotlin_type {
            KotlinType::Class { fqn, .. } => assert_eq!(fqn, "kotlin.String"),
            _ => panic!("Expected Class type"),
        }
    }

    #[test]
    fn test_parse_generic_type_from_ast() {
        let source = "val list: List<String> = emptyList()";
        let parse_tree = crate::parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_type_ref(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::TYPE_REF {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_type_ref(&child) {
                    return Some(found);
                }
            }
            None
        }

        let type_ref = find_type_ref(&root).expect("TYPE_REF not found");
        let kotlin_type = KotlinType::from_ast(&type_ref);

        assert_eq!(kotlin_type.to_string(), "List<String>");
    }

    #[test]
    fn test_parse_nested_generic_from_ast() {
        let source = "val map: Map<String, List<Int>> = emptyMap()";
        let parse_tree = crate::parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_type_ref(node: &SyntaxNode) -> Option<SyntaxNode> {
            if node.kind() == SyntaxKind::TYPE_REF {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_type_ref(&child) {
                    return Some(found);
                }
            }
            None
        }

        let type_ref = find_type_ref(&root).expect("TYPE_REF not found");
        let kotlin_type = KotlinType::from_ast(&type_ref);

        assert_eq!(kotlin_type.to_string(), "Map<String, List<Int>>");
    }

    #[test]
    fn test_parse_nullable_type_from_ast() {
        let source = "val x: String? = null";
        let parse_tree = crate::parse(source);
        let root = SyntaxNode::new_root(parse_tree.green_node);

        fn find_type_node(node: &SyntaxNode) -> Option<SyntaxNode> {
            if matches!(
                node.kind(),
                SyntaxKind::TYPE_REF | SyntaxKind::NULLABLE_TYPE
            ) {
                return Some(node.clone());
            }
            for child in node.children() {
                if let Some(found) = find_type_node(&child) {
                    return Some(found);
                }
            }
            None
        }

        let type_node = find_type_node(&root).expect("Type node not found");
        let kotlin_type = KotlinType::from_ast(&type_node);

        assert_eq!(kotlin_type.to_string(), "String?");
        assert!(kotlin_type.is_nullable());
    }
}
