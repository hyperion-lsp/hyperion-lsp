// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Core types for scope analysis
//!
//! This module contains the fundamental types used throughout the scope system:
//! - ScopeSymbol: represents a symbol defined in a scope
//! - MethodCallInfo: captures method call context for deferred type resolution
//! - ImportInfo: represents import statements
//! - SmartCast: represents a type narrowing from an `is` check

use crate::types::KotlinType;
use rowan::TextRange;

/// A symbol defined in a scope
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSymbol {
    /// Name of the symbol
    pub name: String,

    /// Kind of symbol (variable, parameter, function, class, etc.)
    pub kind: ScopeSymbolKind,

    /// Text range where the symbol is defined
    pub definition_range: TextRange,

    /// Optional type annotation (using the unified KotlinType)
    pub type_annotation: Option<KotlinType>,

    /// Method call information (if this symbol is assigned from a method call)
    pub method_call_info: Option<MethodCallInfo>,
}

/// Byte offset in the source file
pub type Offset = usize;

/// Information about a method call used in variable initialization
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodCallInfo {
    /// Receiver name (None for function calls, Some("user") for user.getName())
    pub receiver_name: Option<String>,

    /// Method or function name
    pub method_name: String,

    /// True if this is a chained call (e.g., a.getB().getC())
    pub is_chained: bool,

    /// For chained calls, the type of the receiver (if determinable from literals/constructors)
    /// Example: "hello".uppercase().length -> receiver_type = Some(KotlinType::simple("String"))
    pub receiver_type: Option<KotlinType>,

    /// Argument tokens for generic type inference
    /// Tokens between the parentheses of a function call
    pub arg_tokens: Option<Vec<(crate::SyntaxKind, String)>>,
}

/// Kind of scope symbol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeSymbolKind {
    /// Local variable (val/var)
    Variable,

    /// Function parameter
    Parameter,

    /// Local function
    Function,

    /// Local class
    Class,

    /// Lambda parameter
    LambdaParameter,

    /// Loop variable (for loops)
    LoopVariable,
}

/// Kind of import statement
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// Explicit import: import kotlin.collections.List
    Explicit,

    /// Wildcard import: import kotlin.collections.*
    Wildcard,

    /// Aliased import: import kotlin.collections.List as L
    Aliased { alias: String },
}

/// Information about an import statement
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportInfo {
    /// Simple name being imported (e.g., "List")
    pub name: String,

    /// Fully qualified name (e.g., "kotlin.collections.List")
    pub fqn: String,

    /// Type of import
    pub kind: ImportKind,

    /// Text range of the import statement
    pub definition_range: TextRange,
}

/// A smart cast that narrows a variable's type within a scope
///
/// Created when processing patterns like:
/// ```kotlin
/// if (x is String) {
///     x.length  // x is String here
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartCast {
    /// Name of the variable being narrowed
    pub variable_name: String,

    /// The narrowed type (e.g., "String" for `x is String`)
    pub narrowed_type: KotlinType,
}
