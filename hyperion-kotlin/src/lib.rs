// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Production analysis paths must handle failure explicitly. Tests intentionally
// use unwrap when reaching the asserted value is itself the test contract.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![allow(clippy::redundant_closure, clippy::use_self)]

//! Kotlin parsing and semantic building blocks for Hyperion.
//!
//! The crate provides a Logos lexer, a lossless Rowan concrete syntax tree,
//! symbol and scope extraction, type inference, smart casts, extension
//! resolution and `when` exhaustiveness analysis. LSP request routing lives in
//! `hyperion-lsp`; this crate supplies the Kotlin-specific analysis it calls.
//!
//! The parser accepts the checked-in conformance corpus without errors and
//! round-trips it losslessly. That corpus is a regression floor, not a claim of
//! complete Kotlin grammar or semantic compatibility. See the generated matrix
//! in the repository README for its exact scope.
//!
//! Kotlin support is intentionally not described as feature-complete here:
//! parsing a construct and resolving its runtime or compiler semantics are
//! separate capabilities. The root README is the authority for user-visible
//! LSP coverage and known gaps.

/// Syntax tree node types (SyntaxKind enum)
pub mod syntax;

/// Lexical analysis module
pub mod lexer;

/// Parsing module
pub mod parser;

/// Symbol extraction module
pub mod symbol_extractor;

/// Scope analysis module
pub mod scope;

/// Type extraction module
pub mod type_extractor;

/// Enhanced type system for generics support
pub mod types;

/// AST helper types
pub mod ast;

/// Standard library function signatures
pub mod stdlib;

/// Type inference engine
pub mod inference;

/// User-defined generic method registry
pub mod user_registry;

/// When expression exhaustiveness checking for sealed classes
pub mod when_exhaustiveness;

/// Smart cast tracking for type narrowing after is checks
pub mod smart_cast;

/// Extension function registry and scope resolution
pub mod extension_registry;

/// Members and extensions of the Kotlin standard library (completion)
pub mod stdlib_members;

/// The Java-callable view of Kotlin declarations (file facades, accessors, statics)
pub mod jvm_view;

// Re-export commonly used types
pub use extension_registry::{is_extension_in_scope, ExtensionMatch};
pub use inference::{
    infer_generic_call, infer_lambda_it_type, infer_method_return_type,
    infer_method_return_type_with_registry, LambdaContext, TypeUnifier,
};
pub use lexer::Lexer;
pub use parser::{parse, ParseError, ParseTree, Parser};
pub use scope::{
    build_scope_graph, ImportInfo, ImportKind, MethodCallInfo, ScopeGraph, ScopeSymbol,
    ScopeSymbolKind, SmartCast,
};
pub use smart_cast::{
    analyze_condition, analyze_when_branches, ConditionAnalysis, SmartCastContext, SmartCastInfo,
    SmartCastSource,
};
pub use stdlib::{GenericFunctionSignature, ParameterSig, StdlibRegistry, TypeExpr, STDLIB};
pub use symbol_extractor::{
    ConstructorParam, ExtractionResult, ImportMap, Modifiers, Symbol, SymbolExtractor, TextRange,
    TypeAlias, TypeParameterInfo,
};
pub use syntax::{KotlinLanguage, SyntaxKind, SyntaxNode, SyntaxToken};
pub use type_extractor::{resolve_method_return_type, Type, TypeExtractor};
pub use types::{KotlinType, Nullability, TypeArgument, TypeParameter, Variance};
pub use user_registry::{
    build_registry_from_symbols, UserGenericClass, UserGenericRegistry, UserMethodSignature,
};
pub use when_exhaustiveness::{
    check_exhaustiveness, extract_when_info, find_when_expressions, ExhaustivenessCheckResult,
    WhenExpressionInfo,
};

/// Every public declaration name in the Kotlin standard library (functions,
/// properties, classes, typealiases), generated from `kotlin-stdlib` 2.2 sources.
///
/// Used to decide whether an unresolved-looking bare name may come from the
/// default imports (`kotlin.*`, `kotlin.collections.*`, ...). Over-inclusion is
/// deliberate: it only ever suppresses a diagnostic.
pub fn is_kotlin_stdlib_name(name: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::OnceLock;
    static NAMES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    NAMES
        .get_or_init(|| include_str!("stdlib_names.txt").lines().collect())
        .contains(name)
}
