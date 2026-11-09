// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Groovy analysis with a Gradle DSL focus.
//!
//! Hyperion parses Groovy through tree-sitter and builds a typed internal AST
//! for Gradle scripts, closures, method calls, GStrings and related constructs.
//! It also provides Gradle API metadata, Groovy extension-method resolution,
//! MetaClass support and symbol/highlighting facts for the LSP layer.
//!
//! The product goal is reliable build-script understanding, not complete
//! Groovy compiler semantics. Runtime metaprogramming and arbitrary AST
//! transforms remain conservative boundaries. User-visible support is
//! documented in the repository README.

/// Gradle API method definitions and signatures
pub mod gradle_api;

/// Symbol extraction for Gradle DSL
pub mod symbol_extractor;

/// Tree-sitter Groovy parser wrapper
pub mod tree_sitter_wrapper;

/// AST helper types
pub mod ast;

/// MetaClass system and registry
pub mod meta_class;

/// Method resolution using Groovy's Meta-Object Protocol
pub mod method_resolver;

/// Parser-neutral semantic highlighting facts for LSP adapters.
pub mod semantic_highlight;

/// Groovy closure semantics and context
pub mod closure;

/// DefaultGroovyMethods (DGMM) extension registry
pub mod extension_registry;

/// GString (Groovy String) interpolation support
pub mod gstring;

/// Groovy Categories and Mixins support
pub mod categories;

/// AST builder - converts tree-sitter CST to typed AST
pub mod ast_builder;

/// LSP-compatible symbol extractor (AST-based)
pub mod lsp_symbol_extractor;

// Re-export commonly used types
pub use ast::{
    CatchBlock, ClosureExpr, Credentials, Dependency, Expression, GStringPart, GradleBlock,
    MethodArgument, MethodCall, Parameter, Repository, Statement, SwitchCase,
};
pub use ast_builder::AstBuilder;
pub use categories::{
    apply_category, apply_mixin, Category, CategoryRegistry, MixinMetadata, MixinRegistry,
};
pub use closure::{
    ClosureContext, ClosureParameter, GroovyClosure, ResolveStrategy, SourceLocation,
};
pub use extension_registry::ExtensionRegistry;
pub use gradle_api::{GradleMethod, GradleMethodSignature, GRADLE_API};
pub use gstring::{GStringEvaluator, ParsedGString};
pub use lsp_symbol_extractor::LspSymbolExtractor;
pub use meta_class::{MetaClass, MetaClassRegistry, MetaMethod, MetaProperty};
pub use method_resolver::{ArgumentTypes, MethodCandidate, MethodResolution, MethodResolver};
pub use symbol_extractor::{GradleSymbol, SymbolExtractor};
pub use tree_sitter_wrapper::{
    BinaryOpNode, ClosureNode, GStringNode, MethodCallNode, PropertyAccessNode,
};

#[cfg(test)]
mod tests {
    #[test]
    fn gradle_api_loads() {
        // The shipped Gradle catalog must contain at least one usable entry.
        assert!(!crate::GRADLE_API.is_empty());
    }
}
