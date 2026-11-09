// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! # Hyperion Semantic Database
//!
//! This module defines the core database infrastructure for Hyperion's semantic analysis layer.
//! It uses the Salsa incremental computation framework to provide efficient, on-demand analysis
//! with automatic change propagation and memoization.
//!
//! ## Critical: File Registration Requirements
//!
//! When registering a file with the database, you **MUST** call both:
//! 1. `db.set_file_text(file_id, source)` - Set the file contents
//! 2. `db.set_file_language(file_id, language)` - Set the file language
//!
//! **Failure to call `set_file_language` will cause runtime panics** with the error:
//! `"no value set for FileLanguageQuery(FileId(N))"`
//!
//! Prefer using the helper method [`Database::register_file`] which handles both:
//!
//! ```ignore
//! // Correct - use register_file helper
//! db.register_file(file_id, source, Language::Java);
//!
//! // Also correct - manual setup
//! db.set_file_text(file_id, source);
//! db.set_file_language(file_id, Language::Java);
//!
//! // Incorrect - will panic on any query!
//! db.set_file_text(file_id, source);
//! // Missing set_file_language!
//! ```
//!
//! ## Architecture Overview
//!
//! The database is organized into multiple query groups, each responsible for a specific layer
//! of semantic analysis:
//!
//! - **SourceDatabase**: Manages raw source file contents (inputs)
//! - **ParseDatabase**: Handles parsing source code into syntax trees
//! - **SemanticDatabase**: Performs symbol resolution, type inference, and semantic analysis
//!
//! ## Query-Based Computation
//!
//! Salsa enables a query-based architecture where:
//! - **Input queries** represent raw data (e.g., file contents)
//! - **Derived queries** compute results based on other queries
//! - **Memoization** caches results automatically
//! - **Incremental recomputation** only recalculates what changed
//!
//! ## Example Usage
//!
//! ```ignore
//! use hyperion_semantics::db::{Database, SourceDatabase, ParseDatabase, SemanticDatabase};
//!
//! let mut db = Database::default();
//!
//! // Register file with text AND language (required!)
//! let file_id = FileId::new(0);
//! db.register_file(file_id, Arc::new("public class Hello {}".to_string()), Language::Java);
//!
//! // Query derived data: parse the file
//! let parse_tree = db.parse_file(file_id);
//!
//! // Query semantic data: get symbols
//! let symbols = db.file_symbols(file_id);
//! ```

mod nullability_query;

use crate::file_semantics::FileSemantics;
use crate::resolver::ResolverBuilder;
use crate::semantic_walker::compute_file_semantics;
use crate::symbol::SymbolTable;
use hyperion_parser::{Lexer, Parser, SyntaxKind, SyntaxNode};
use hyperion_workspace::{WorkspaceIndex, WorkspaceRevision};
use rowan::GreenNode;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::Arc;

// Re-export FileId and Language from workspace for consistency
pub use hyperion_workspace::{FileId, Language};

/// A Salsa-compatible handle to a `WorkspaceIndex` behind an `Arc`.
///
/// Salsa input queries require `Eq` + `Hash`. Since the `WorkspaceIndex` itself
/// is large, we use pointer equality: the handle never changes (same Arc),
/// and a separate `workspace_version` input is bumped to force recomputation.
#[derive(Clone)]
pub struct WorkspaceIndexHandle(pub Arc<WorkspaceIndex>);

impl std::fmt::Debug for WorkspaceIndexHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("WorkspaceIndexHandle")
            .field(&Arc::as_ptr(&self.0))
            .finish()
    }
}

impl PartialEq for WorkspaceIndexHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for WorkspaceIndexHandle {}

impl Hash for WorkspaceIndexHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

/// Represents a parsed syntax tree for a source file.
///
/// Stores the immutable GreenNode from rowan, which can be converted to a
/// SyntaxNode (red tree) on demand for traversal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseTree {
    /// The root green node (immutable, structural-shared CST)
    pub green: GreenNode,
    /// Parse errors encountered (for error recovery and diagnostics)
    pub errors: Arc<Vec<hyperion_parser::ParseError>>,
}

impl ParseTree {
    /// Converts the green tree to a red tree for traversal.
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }
}

/// Represents a symbol in the source code (class, method, field, variable, etc.).
///
/// Symbols are the fundamental unit of semantic analysis, representing named
/// entities that can be referenced and resolved.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Symbol {
    /// The name of the symbol
    pub name: Arc<str>,
    /// The kind of symbol (class, method, field, etc.)
    pub kind: SymbolKind,
    /// The file where this symbol is defined
    pub file: FileId,
    /// Byte offset in the file where the symbol is defined
    pub offset: u32,
    /// Whether this symbol is marked with @Deprecated annotation
    pub is_deprecated: bool,
}

/// Kinds of symbols that can appear in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Class,
    Interface,
    Enum,
    Method,
    Field,
    Variable,
    Parameter,
    Package,
}

impl From<lsp_types::SymbolKind> for SymbolKind {
    fn from(kind: lsp_types::SymbolKind) -> Self {
        match kind {
            lsp_types::SymbolKind::INTERFACE => Self::Interface,
            lsp_types::SymbolKind::ENUM => Self::Enum,
            lsp_types::SymbolKind::METHOD | lsp_types::SymbolKind::CONSTRUCTOR => Self::Method,
            lsp_types::SymbolKind::FIELD
            | lsp_types::SymbolKind::PROPERTY
            | lsp_types::SymbolKind::CONSTANT
            | lsp_types::SymbolKind::ENUM_MEMBER => Self::Field,
            lsp_types::SymbolKind::VARIABLE => Self::Variable,
            lsp_types::SymbolKind::PACKAGE | lsp_types::SymbolKind::MODULE => Self::Package,
            _ => Self::Class, // conservative fallback
        }
    }
}

/// A collection of symbols found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSymbols {
    pub symbols: Arc<Vec<Symbol>>,
}

impl FileSymbols {
    /// Creates an empty symbol collection.
    pub fn empty() -> Self {
        Self {
            symbols: Arc::new(Vec::new()),
        }
    }

    /// Creates a symbol collection from a vector of symbols.
    pub fn new(symbols: Vec<Symbol>) -> Self {
        Self {
            symbols: Arc::new(symbols),
        }
    }
}

// =============================================================================
// Database Query Groups
// =============================================================================

/// Source database query group.
///
/// This query group manages the input layer of the database - raw source file contents.
/// These are the only queries that are directly set by the LSP server or workspace manager.
///
/// All other queries are derived from these inputs through the query dependency graph.
#[allow(clippy::trait_duplication_in_bounds)]
#[salsa::query_group(SourceDatabaseStorage)]
pub trait SourceDatabase: salsa::Database {
    /// Returns the text contents of a file.
    ///
    /// This is an **input query** - its value must be explicitly set via `set_file_text`.
    /// Changes to file contents automatically invalidate all derived queries that depend
    /// on this file.
    ///
    /// # Example
    ///
    /// ```ignore
    /// db.set_file_text(file_id, "class Foo {}".to_string());
    /// let text = db.file_text(file_id);
    /// assert_eq!(text.as_ref(), "class Foo {}");
    /// ```
    #[salsa::input]
    fn file_text(&self, file: FileId) -> Arc<String>;

    /// Returns the language for a file.
    ///
    /// This is an **input query** - its value must be explicitly set via `set_file_language`.
    /// Querying a file whose language was not set is a programming error and will panic.
    ///
    /// # Example
    ///
    /// ```ignore
    /// db.set_file_language(file_id, Language::Kotlin);
    /// let lang = db.file_language(file_id);
    /// assert_eq!(lang, Language::Kotlin);
    /// ```
    #[salsa::input]
    fn file_language(&self, file: FileId) -> Language;
}

/// Parse database query group.
///
/// This query group handles parsing source code into syntax trees (CSTs/ASTs).
/// Parsing is incremental - when a file changes, only that file is reparsed,
/// and the parse is memoized for future queries.
#[salsa::query_group(ParseDatabaseStorage)]
pub trait ParseDatabase: SourceDatabase {
    /// Parses a file into a syntax tree.
    ///
    /// This is a **derived query** - it automatically computes the parse tree from
    /// the file's text contents. The result is memoized, so repeated calls with the
    /// same file return the cached tree without reparsing.
    ///
    /// When `file_text(file)` changes, this query is automatically invalidated and
    /// will reparse on the next access.
    ///
    /// # Example
    ///
    /// ```ignore
    /// db.set_file_text(file_id, "class Bar {}".to_string());
    /// let tree = db.parse_file(file_id); // Parses the file
    /// let tree2 = db.parse_file(file_id); // Returns cached result
    /// ```
    fn parse_file(&self, file: FileId) -> Arc<ParseTree>;
}

/// Semantic database query group.
///
/// This query group performs semantic analysis: symbol resolution, type checking,
/// scope analysis, and other high-level operations that require understanding the
/// meaning of the code.
///
/// ## Type Inference Caching Strategy
///
/// Type inference in Hyperion is **implicitly incremental** through Salsa's query system:
///
/// - **Parse Trees** (`parse_file`): Cached per file, invalidated on text changes
/// - **Symbol Tables** (`file_symbol_table`): Cached per file with full scope hierarchy
/// - **TypeInferenceEngine**: Stateless, delegates to cached queries above
/// - **WorkspaceIndex**: Caches cross-file symbol lookups in memory
///
/// This layered caching strategy means type inference automatically benefits from
/// incrementality without needing explicit expression-level caching. When a file changes:
///
/// 1. Only that file's parse tree and symbol table are recomputed
/// 2. Type inference reuses cached data from unchanged files
/// 3. Expensive operations (parsing, symbol extraction) run once per edit
///
/// Adding fine-grained expression-level caching would add complexity without significant
/// benefit, since expression offsets are unstable across edits and type inference is
/// already fast (just AST walking + cached lookups).
#[salsa::query_group(SemanticDatabaseStorage)]
pub trait SemanticDatabase: ParseDatabase {
    /// Returns the current method signature version.
    ///
    /// This is an **input query** that tracks cross-file cache invalidation for
    /// nullability analysis. When method signatures change (e.g., adding @Nullable
    /// annotation), this version is incremented to invalidate dependent analyses.
    ///
    /// The version is synchronized with [`MethodSignatureCache`] and bumped via
    /// [`Database::bump_method_signature_version`].
    ///
    /// # Example
    ///
    /// ```ignore
    /// // When a method signature changes
    /// db.bump_method_signature_version();
    ///
    /// // Dependent queries (like file_nullability) will rerun
    /// let analysis = db.file_nullability(dependent_file);
    /// ```
    #[salsa::input]
    fn method_signature_version(&self) -> u64;

    /// Immutable cross-file nullability summaries paired with the current
    /// method-signature version. Unlike the mutable publication cache, this
    /// input is frozen in each Salsa snapshot.
    #[salsa::input]
    fn method_signatures(&self) -> crate::nullability::MethodSignatureSnapshot;

    /// Returns the current workspace version for cache invalidation.
    ///
    /// Bumped when the workspace index changes (files added/removed/modified).
    /// Queries that depend on workspace data read this to establish a Salsa
    /// dependency, ensuring they recompute when the index changes.
    #[salsa::input]
    fn workspace_version(&self) -> u64;

    /// Returns a handle to the workspace index.
    ///
    /// This is an input query set once during initialization. The handle itself
    /// never changes (pointer-eq), so Salsa considers it stable. The actual
    /// invalidation comes from `workspace_version` being bumped.
    #[salsa::input]
    fn workspace_index_handle(&self) -> WorkspaceIndexHandle;

    /// Returns pre-computed name resolution and diagnostics for a file.
    ///
    /// This is a **derived query** that walks the AST and resolves every name
    /// node. Java and Kotlin results are cached against the file inputs and
    /// workspace version. Other languages return empty semantics and do not
    /// depend on the workspace version.
    fn file_semantics(&self, file: FileId) -> Arc<FileSemantics>;

    /// Cache scoped semantics against published module bindings as well as
    /// source text. A classpath commit can precede its version notification.
    fn file_semantics_at_workspace_revision(
        &self,
        file: FileId,
        revision: WorkspaceRevision,
    ) -> Arc<FileSemantics>;
    /// Returns all symbols defined in a file.
    ///
    /// This is a **derived query** that extracts symbol definitions from the parse tree.
    /// It's used for features like document outline, go-to-definition, and workspace
    /// symbol search.
    ///
    /// The query is automatically invalidated when the file's parse tree changes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// db.set_file_text(file_id, "class Foo { void bar() {} }".to_string());
    /// let symbols = db.file_symbols(file_id);
    /// assert_eq!(symbols.symbols.len(), 2); // Foo and bar
    /// ```
    fn file_symbols(&self, file: FileId) -> Arc<FileSymbols>;

    /// Returns a cached symbol table for a file.
    ///
    /// This is a **derived query** that builds a full scope-aware symbol table from the
    /// parse tree. Unlike `file_symbols` which only extracts top-level declarations, this
    /// table includes all symbols with their scope hierarchy, enabling efficient symbol
    /// resolution without re-walking the AST.
    ///
    /// **Performance**: This query is cached and only recomputed when the file changes.
    /// Use this for LSP features that need scope resolution (completion, hover, etc.)
    /// instead of calling `ResolverBuilder::build_from_syntax` repeatedly.
    ///
    /// **Note**: This table is built without workspace index, so `var` type inference
    /// for complex expressions (method calls) will result in `Type::Unknown`. For features
    /// requiring accurate var types, use `ResolverBuilder::with_workspace` directly.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let table = db.file_symbol_table(file_id);
    /// let resolver = SymbolResolver::with_symbol_table((*table).clone());
    /// let symbol = resolver.resolve_in_scope("myVar", scope_id);
    /// ```
    fn file_symbol_table(&self, file: FileId) -> Arc<SymbolTable>;

    /// Returns the scope graph for a file.
    ///
    /// This is a **derived query** that builds a scope graph from the parse tree.
    /// The scope graph tracks local variables, parameters, and other lexically-scoped
    /// symbols to enable within-file navigation.
    ///
    /// This query is Kotlin-specific. Java, Groovy and Scala use their existing
    /// resolver/index paths and receive an empty Kotlin scope graph.
    ///
    /// The query is automatically invalidated when the file's parse tree changes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// db.set_file_text(file_id, "fun test() { val x = 5 }".to_string());
    /// db.set_file_language(file_id, Language::Kotlin);
    /// let scope_graph = db.scope_graph(file_id);
    /// // Can now resolve 'x' within the function scope
    /// ```
    fn scope_graph(&self, file: FileId) -> hyperion_kotlin::ScopeGraph;

    /// Returns nullability analysis results for a file.
    ///
    /// This is a **derived query** that performs forward dataflow analysis
    /// for nullability tracking on all methods in the file. The analysis
    /// detects potential null pointer dereferences, redundant null checks,
    /// and other nullability-related issues.
    ///
    /// **Language support:** Currently only Java files are analyzed. Kotlin
    /// has its own built-in nullability system and is not analyzed here.
    ///
    /// **Performance:** Java results are cached per file and recomputed when
    /// the parse tree or method-signature snapshot changes. Other languages
    /// return empty analysis based only on their language input. The analysis
    /// uses the CFG-based dataflow framework to track nullability through
    /// control flow paths.
    ///
    /// # Example
    ///
    /// ```ignore
    /// db.set_file_text(file_id, "class Test { void f(String s) { s.length(); } }".to_string());
    /// db.set_file_language(file_id, Language::Java);
    /// let analysis = db.file_nullability(file_id);
    /// // Check for potential null dereference diagnostics
    /// for diag in &analysis.diagnostics {
    ///     println!("{:?}", diag.kind);
    /// }
    /// ```
    fn file_nullability(&self, file: FileId) -> Arc<crate::nullability::FileNullabilityAnalysis>;
}

// =============================================================================
// Query Implementations
// =============================================================================

/// Implementation of the `parse_file` query.
///
/// This function is called by Salsa when the parse tree for a file is requested
/// and not already cached. It parses the source text into a syntax tree using
/// hyperion-parser's lexer and parser.
///
/// The parser builds a lossless concrete syntax tree (CST) using rowan, which
/// preserves all whitespace, comments, and structural information for features
/// like formatting, refactoring, and incremental reparsing.
fn parse_file(db: &dyn ParseDatabase, file: FileId) -> Arc<ParseTree> {
    let text = db.file_text(file);
    let language = db.file_language(file);

    tracing::debug!(
        "Parsing {:?} file {:?} ({} bytes)",
        language,
        file,
        text.len()
    );

    // Dispatch to appropriate parser based on language
    let (green, errors) = match language {
        Language::Kotlin => {
            // Use Kotlin parser
            let tokens: Vec<_> = hyperion_kotlin::Lexer::new(text.as_ref()).collect();
            tracing::trace!("Lexed {} Kotlin tokens for file {:?}", tokens.len(), file);

            let (green, kt_errors) = hyperion_kotlin::Parser::parse(tokens);

            // Convert Kotlin ParseError to Java ParseError format
            let errors: Vec<_> = kt_errors
                .into_iter()
                .map(|e| hyperion_parser::ParseError {
                    message: e.message,
                    range: rowan::TextRange::new(e.range.start(), e.range.end()),
                })
                .collect();

            (green, errors)
        }
        Language::Java => {
            // Use Java parser (default)
            let tokens: Vec<_> = Lexer::new(text.as_ref()).collect();
            tracing::trace!("Lexed {} Java tokens for file {:?}", tokens.len(), file);
            Parser::parse(tokens)
        }
        Language::Groovy => {
            // Groovy uses tree-sitter parser separately
            // Return a minimal valid tree - symbol extraction happens via tree-sitter
            tracing::debug!("Groovy file {:?} - using tree-sitter for parsing", file);
            (empty_source_file(), vec![])
        }
        Language::Scala => {
            // Scala uses tree-sitter-based extraction (Phase 1)
            // Return a minimal valid parse tree - symbol extraction happens via tree-sitter
            tracing::debug!("Scala file {:?} - using tree-sitter extraction", file);
            (empty_source_file(), vec![])
        }
    };

    if !errors.is_empty() {
        tracing::warn!("Parse errors in file {:?}: {} errors", file, errors.len());
    }

    Arc::new(ParseTree {
        green,
        errors: Arc::new(errors),
    })
}

fn empty_source_file() -> rowan::GreenNode {
    let mut builder = rowan::GreenNodeBuilder::new();
    builder.start_node(rowan::SyntaxKind(SyntaxKind::SOURCE_FILE as u16));
    builder.finish_node();
    builder.finish()
}

/// Check if a node has a @Deprecated annotation
fn has_deprecated_annotation(node: &SyntaxNode) -> bool {
    use rowan::NodeOrToken;

    // Check immediate children for ANNOTATION nodes
    for child in node.children() {
        if child.kind() == SyntaxKind::ANNOTATION {
            // Check if annotation name is "Deprecated"
            for token in child.descendants_with_tokens() {
                if let NodeOrToken::Token(t) = token {
                    if t.kind() == SyntaxKind::IDENT && t.text() == "Deprecated" {
                        return true;
                    }
                }
            }
        }
        // Also check MODIFIER nodes which may contain annotations
        if child.kind() == SyntaxKind::MODIFIER {
            for annotation in child.children() {
                if annotation.kind() == SyntaxKind::ANNOTATION {
                    for token in annotation.descendants_with_tokens() {
                        if let NodeOrToken::Token(t) = token {
                            if t.kind() == SyntaxKind::IDENT && t.text() == "Deprecated" {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }

    false
}

/// Implementation of the `file_symbols` query.
///
/// This function extracts symbol definitions from a parsed syntax tree.
/// It walks the Rowan CST and collects named type, method and field
/// declarations, including nested declarations. The compact `Symbol` result
/// retains name, kind, file, offset and deprecation state; richer scope and
/// type data belongs to `file_symbol_table`.
///
/// Local variables are deliberately excluded from this file-level outline.
fn file_symbols(db: &dyn SemanticDatabase, file: FileId) -> Arc<FileSymbols> {
    let parse_tree = db.parse_file(file);

    tracing::debug!("Extracting symbols from file {:?}", file);

    let mut symbols = Vec::new();

    let root = parse_tree.syntax();

    // Descendant traversal includes member and nested type declarations.
    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::CLASS_DECL => {
                // Find the identifier token for the class name
                if let Some(name_token) = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind() == SyntaxKind::IDENT)
                {
                    let name = name_token.text();
                    let offset = u32::from(node.text_range().start());

                    symbols.push(Symbol {
                        name: Arc::from(name),
                        kind: SymbolKind::Class,
                        file,
                        offset,
                        is_deprecated: has_deprecated_annotation(&node),
                    });

                    tracing::trace!("Found class '{}' at offset {}", name, offset);
                }
            }
            SyntaxKind::INTERFACE_DECL => {
                // Find the identifier token for the interface name
                if let Some(name_token) = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind() == SyntaxKind::IDENT)
                {
                    let name = name_token.text();
                    let offset = u32::from(node.text_range().start());

                    symbols.push(Symbol {
                        name: Arc::from(name),
                        kind: SymbolKind::Interface,
                        file,
                        offset,
                        is_deprecated: has_deprecated_annotation(&node),
                    });

                    tracing::trace!("Found interface '{}' at offset {}", name, offset);
                }
            }
            SyntaxKind::ENUM_DECL => {
                // Find the identifier token for the enum name
                if let Some(name_token) = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind() == SyntaxKind::IDENT)
                {
                    let name = name_token.text();
                    let offset = u32::from(node.text_range().start());

                    symbols.push(Symbol {
                        name: Arc::from(name),
                        kind: SymbolKind::Enum,
                        file,
                        offset,
                        is_deprecated: has_deprecated_annotation(&node),
                    });

                    tracing::trace!("Found enum '{}' at offset {}", name, offset);
                }
            }
            SyntaxKind::RECORD_DECL => {
                if let Some(name_token) = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind() == SyntaxKind::IDENT)
                {
                    let name = name_token.text();
                    let offset = u32::from(node.text_range().start());

                    symbols.push(Symbol {
                        name: Arc::from(name),
                        kind: SymbolKind::Class,
                        file,
                        offset,
                        is_deprecated: has_deprecated_annotation(&node),
                    });

                    tracing::trace!("Found record '{}' at offset {}", name, offset);
                }
            }
            SyntaxKind::ANNOTATION_DECL => {
                if let Some(name_token) = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind() == SyntaxKind::IDENT)
                {
                    let name = name_token.text();
                    let offset = u32::from(node.text_range().start());

                    symbols.push(Symbol {
                        name: Arc::from(name),
                        kind: SymbolKind::Interface,
                        file,
                        offset,
                        is_deprecated: has_deprecated_annotation(&node),
                    });

                    tracing::trace!("Found annotation '{}' at offset {}", name, offset);
                }
            }
            SyntaxKind::METHOD_DECL => {
                // Find the identifier token for the method name
                if let Some(name_token) = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind() == SyntaxKind::IDENT)
                {
                    let name = name_token.text();
                    let offset = u32::from(node.text_range().start());

                    symbols.push(Symbol {
                        name: Arc::from(name),
                        kind: SymbolKind::Method,
                        file,
                        offset,
                        is_deprecated: has_deprecated_annotation(&node),
                    });

                    tracing::trace!("Found method '{}' at offset {}", name, offset);
                }
            }
            SyntaxKind::FIELD_DECL => {
                // Find the identifier token for the field name
                if let Some(name_token) = node
                    .children_with_tokens()
                    .filter_map(rowan::NodeOrToken::into_token)
                    .find(|token| token.kind() == SyntaxKind::IDENT)
                {
                    let name = name_token.text();
                    let offset = u32::from(node.text_range().start());

                    symbols.push(Symbol {
                        name: Arc::from(name),
                        kind: SymbolKind::Field,
                        file,
                        offset,
                        is_deprecated: has_deprecated_annotation(&node),
                    });

                    tracing::trace!("Found field '{}' at offset {}", name, offset);
                }
            }
            _ => {}
        }
    }

    tracing::debug!("Extracted {} symbols from file {:?}", symbols.len(), file);

    Arc::new(FileSymbols::new(symbols))
}

/// Implementation of the `file_symbol_table` query.
///
/// Builds a full scope-aware symbol table from the parse tree. This is more
/// comprehensive than `file_symbols` as it includes the complete scope hierarchy
/// needed for symbol resolution.
///
/// The table is built without workspace index, so `var` type inference for
/// complex expressions will return `Type::Unknown`. This is acceptable because:
/// 1. Most LSP features only need scope structure, not precise var types
/// 2. Features needing var types can still use ResolverBuilder::with_workspace
fn file_symbol_table(db: &dyn SemanticDatabase, file: FileId) -> Arc<SymbolTable> {
    let parse_tree = db.parse_file(file);
    let language = db.file_language(file);

    tracing::debug!("Building symbol table for {:?} file {:?}", language, file);

    let table = match language {
        Language::Kotlin => {
            // Use Kotlin resolver for Kotlin files
            let kotlin_root: hyperion_kotlin::SyntaxNode =
                hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());

            let builder = crate::kotlin_resolver::KotlinResolverBuilder::new(file);
            builder.build(&kotlin_root)
        }
        Language::Java => {
            // Use Java resolver for Java files
            let root = parse_tree.syntax();
            let builder = ResolverBuilder::new(file);
            builder.build_symbol_table(&root)
        }
        Language::Groovy => {
            // Groovy symbol extraction uses tree-sitter separately
            // Return an empty symbol table for now
            tracing::debug!("Groovy file {:?} - symbol table via tree-sitter", file);
            crate::SymbolTable::new()
        }
        Language::Scala => {
            // Scala declarations are indexed by the workspace's CST extractor;
            // this Java/Kotlin semantic table therefore stays empty.
            tracing::debug!("Scala file {:?} - symbol table via workspace index", file);
            crate::SymbolTable::new()
        }
    };

    let stats = table.statistics();
    tracing::debug!(
        "Built symbol table for file {:?}: {} symbols, {} scopes",
        file,
        stats.symbol_count,
        stats.scope_count
    );

    Arc::new(table)
}

/// Implementation of the `scope_graph` query.
///
/// This function builds a scope graph from the parsed syntax tree.
/// The scope graph is used for local symbol resolution within a file.
///
/// This query is Kotlin-specific. Other languages return an empty graph and
/// use their existing resolver/index paths.
fn scope_graph(db: &dyn SemanticDatabase, file: FileId) -> hyperion_kotlin::ScopeGraph {
    let parse_tree = db.parse_file(file);
    let language = db.file_language(file);

    tracing::debug!("Building scope graph for {:?} file {:?}", language, file);

    match language {
        Language::Kotlin => {
            // Convert to Kotlin SyntaxNode
            let kotlin_root: hyperion_kotlin::SyntaxNode =
                hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());

            // Build scope graph from Kotlin AST
            let scope_graph = hyperion_kotlin::build_scope_graph(&kotlin_root);

            tracing::debug!("Built Kotlin scope graph for file {:?}", file);
            scope_graph
        }
        Language::Java | Language::Groovy | Language::Scala => {
            // No scope graph is built for these languages yet: return an empty graph
            // with a zero-width root range.
            tracing::debug!(
                "No scope graph for {:?} files, returning empty graph",
                language
            );
            let file_range =
                rowan::TextRange::new(rowan::TextSize::from(0), rowan::TextSize::from(0));
            hyperion_kotlin::ScopeGraph::new(file_range)
        }
    }
}

/// Implementation of the `file_semantics` query.
///
/// Walks the AST and resolves every name node (CLASS_TYPE, NAME_EXPR, IMPORT_DECL).
/// Returns resolved names and unresolved-symbol diagnostics.
///
/// Runs for Java and Kotlin files. Other languages return empty semantics
/// without depending on the mutable workspace index.
fn file_semantics(db: &dyn SemanticDatabase, file: FileId) -> Arc<FileSemantics> {
    if !matches!(db.file_language(file), Language::Java | Language::Kotlin) {
        return Arc::new(FileSemantics::empty());
    }
    let _version = db.workspace_version();
    let workspace_handle = db.workspace_index_handle();
    // Resolve against the requesting source set's classpath: main sources must
    // not see test-only dependencies, and a module only sees the modules and
    // JARs it depends on. Navigation and diagnostics must agree on this view.
    let scoped = workspace_handle.0.scoped_view_for_file(file);
    let index = scoped
        .as_ref()
        .filter(|view| view.source_scope_is_current())
        .unwrap_or(&workspace_handle.0);
    compute_semantics_with_index(db, file, index)
}

fn file_semantics_at_workspace_revision(
    db: &dyn SemanticDatabase,
    file: FileId,
    revision: WorkspaceRevision,
) -> Arc<FileSemantics> {
    if !matches!(db.file_language(file), Language::Java | Language::Kotlin) {
        return Arc::new(FileSemantics::empty());
    }
    let workspace_handle = db.workspace_index_handle();
    let index = &workspace_handle.0;
    if !index.workspace_bindings_are_current(revision) {
        return Arc::new(FileSemantics::empty());
    }
    let scoped = index.scoped_view_for_file(file);
    let index = scoped.as_ref().unwrap_or(index);
    if !index.source_scope_is_current() {
        return Arc::new(FileSemantics::empty());
    }
    compute_semantics_with_index(db, file, index)
}

fn compute_semantics_with_index(
    db: &dyn SemanticDatabase,
    file: FileId,
    workspace_index: &WorkspaceIndex,
) -> Arc<FileSemantics> {
    let language = db.file_language(file);
    if !matches!(language, Language::Java | Language::Kotlin) {
        return Arc::new(FileSemantics::empty());
    }

    let parse_tree = db.parse_file(file);

    match language {
        Language::Java => {
            let symbol_table = db.file_symbol_table(file);
            Arc::new(compute_file_semantics(
                &parse_tree.syntax(),
                file,
                &symbol_table,
                workspace_index,
            ))
        }
        Language::Kotlin => {
            let kotlin_root = hyperion_kotlin::SyntaxNode::new_root(parse_tree.green.clone());
            let symbol_table = db.file_symbol_table(file);
            Arc::new(
                crate::kotlin_semantic_walker::compute_kotlin_file_semantics(
                    &kotlin_root,
                    file,
                    symbol_table,
                    workspace_index,
                ),
            )
        }
        _ => Arc::new(FileSemantics::empty()),
    }
}

/// Implementation of the `file_nullability` Salsa query.
///
/// The analysis lives in its own module; this root-level function keeps the
/// query implementation name expected by `SemanticDatabase`.
fn file_nullability(
    db: &dyn SemanticDatabase,
    file: FileId,
) -> Arc<crate::nullability::FileNullabilityAnalysis> {
    nullability_query::file_nullability(db, file)
}

// =============================================================================
// Database Implementation
// =============================================================================

/// The main database struct that implements all query groups.
///
/// This struct holds all the state for incremental semantic analysis. It combines
/// Salsa's runtime (for query tracking and memoization) with storage for each query group.
///
/// The database is designed to be long-lived - typically one instance per LSP workspace.
/// It accumulates cached results over time and efficiently handles incremental updates.
///
/// # Thread Safety
///
/// The database supports concurrent queries through Salsa's built-in parallelism support.
/// Multiple threads can query the database simultaneously, with Salsa handling synchronization.
///
/// However, mutations (setting input queries) require exclusive access through snapshot isolation.
#[salsa::database(SourceDatabaseStorage, ParseDatabaseStorage, SemanticDatabaseStorage)]
pub struct Database {
    storage: salsa::Storage<Self>,
    method_signature_cache: Arc<crate::nullability::MethodSignatureCache>,
    /// Persistent registry paired with Salsa's input storage. Cloning it for a
    /// request snapshot is O(1), while later edits structurally share the
    /// unchanged entries without leaking into the older snapshot.
    registered_file_text: im::HashMap<FileId, Arc<String>>,
}

impl Database {
    /// Creates a new database instance.
    ///
    /// The database starts empty - no files are registered. Use [`register_file`](Self::register_file)
    /// to add files.
    pub fn new() -> Self {
        let mut db = Self {
            storage: salsa::Storage::default(),
            method_signature_cache: Arc::new(crate::nullability::MethodSignatureCache::new()),
            registered_file_text: im::HashMap::new(),
        };

        // Initialize input queries
        db.set_method_signature_version(0);
        db.set_method_signatures(crate::nullability::MethodSignatureSnapshot::empty());
        db.set_workspace_version(0);
        // Bound retained results across historical source/classpath revisions.
        FileSemanticsAtWorkspaceRevisionQuery
            .in_db_mut(&mut db)
            .set_lru_capacity(16);
        db.set_workspace_index_handle(WorkspaceIndexHandle(Arc::new(WorkspaceIndex::new(
            PathBuf::from("."),
        ))));

        db
    }

    /// Returns a reference to the method signature cache.
    ///
    /// The cache tracks method signatures across all files for cross-file nullability
    /// analysis. When method signatures change, use [`bump_method_signature_version`]
    /// to invalidate dependent analyses.
    ///
    /// [`bump_method_signature_version`]: Self::bump_method_signature_version
    pub fn method_signature_cache(&self) -> &crate::nullability::MethodSignatureCache {
        &self.method_signature_cache
    }

    /// A shared handle to the member-fact cache. It is interior-locked and
    /// independent of Salsa's revision lock.
    #[must_use]
    pub fn method_signature_cache_handle(&self) -> Arc<crate::nullability::MethodSignatureCache> {
        Arc::clone(&self.method_signature_cache)
    }

    /// Set an authoritative source buffer and remember that this Salsa input is
    /// initialized. Salsa's generated getter panics for unknown inputs, while
    /// LSP cross-file operations need a non-panicking way to prefer open editor
    /// buffers over an older workspace-index snapshot.
    pub fn set_file_text_tracked(&mut self, file: FileId, text: Arc<String>) {
        self.registered_file_text.insert(file, Arc::clone(&text));
        SourceDatabase::set_file_text(self, file, text);
    }

    /// Return a registered source buffer without invoking an unset Salsa input.
    #[must_use]
    pub fn try_file_text(&self, file: FileId) -> Option<Arc<String>> {
        self.registered_file_text.get(&file).cloned()
    }

    /// Bumps the method signature version to invalidate dependent queries.
    ///
    /// This method synchronizes the version between the [`MethodSignatureCache`]
    /// and the Salsa input query. When called, all queries that depend on
    /// `method_signature_version()` will be invalidated and recomputed on next access.
    ///
    /// # Usage
    ///
    /// Call this after updating method signatures in the cache:
    ///
    /// ```ignore
    /// // Update signatures in the cache
    /// let changed = db.method_signature_cache().update_file_signatures(file_id, new_sigs);
    ///
    /// // If signatures changed, bump the version to invalidate dependent queries
    /// if changed {
    ///     db.bump_method_signature_version();
    /// }
    /// ```
    ///
    /// [`MethodSignatureCache`]: crate::nullability::MethodSignatureCache
    pub fn bump_method_signature_version(&mut self) {
        let new_version = self.method_signature_cache.bump_version();
        self.set_method_signature_version(new_version);
        self.set_method_signatures(self.method_signature_cache.snapshot());
    }

    /// Bumps the workspace version to invalidate queries that depend on
    /// workspace index data (e.g., `file_semantics`).
    ///
    /// Call this after the workspace index has been updated (files indexed,
    /// removed, or modified).
    pub fn bump_workspace_version(&mut self) {
        let current = self.workspace_version();
        self.set_workspace_version(current + 1);
    }

    /// Registers a file with the database, setting both text and language.
    ///
    /// This is the **recommended way** to add files to the database. It ensures both
    /// required inputs (`file_text` and `file_language`) are set, preventing runtime
    /// panics from missing language queries.
    ///
    /// # Arguments
    ///
    /// * `file` - The unique identifier for this file
    /// * `text` - The source code contents
    /// * `language` - The programming language (Java or Kotlin)
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut db = Database::new();
    /// let file_id = FileId::new(1);
    /// db.register_file(file_id, Arc::new("class Foo {}".to_string()), Language::Java);
    /// ```
    pub fn register_file(&mut self, file: FileId, text: Arc<String>, language: Language) {
        self.set_file_text_tracked(file, text);
        self.set_file_language(file, language);
    }

    /// Creates a snapshot of the database for concurrent access.
    ///
    /// Snapshots allow read-only access to the database from multiple threads while
    /// the main database continues to accept mutations. This is useful for handling
    /// multiple LSP requests concurrently.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let db = Database::new();
    /// let snapshot = db.snapshot();
    ///
    /// // Use snapshot in another thread
    /// std::thread::spawn(move || {
    ///     let symbols = snapshot.file_symbols(file_id);
    /// });
    /// ```
    pub fn snapshot(&self) -> salsa::Snapshot<Self> {
        salsa::Snapshot::new(Self {
            storage: self.storage.snapshot(),
            method_signature_cache: Arc::clone(&self.method_signature_cache),
            registered_file_text: self.registered_file_text.clone(),
        })
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Salsa's Database trait
impl salsa::Database for Database {}

// Implement ParallelDatabase for concurrent query execution
impl salsa::ParallelDatabase for Database {
    fn snapshot(&self) -> salsa::Snapshot<Self> {
        salsa::Snapshot::new(Self {
            storage: self.storage.snapshot(),
            method_signature_cache: Arc::clone(&self.method_signature_cache),
            registered_file_text: self.registered_file_text.clone(),
        })
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::nullability_query::{resolve_source_member_uses, JavaMethodOwnerResolver};
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_file_text_input() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text_tracked(file, Arc::new("public class Hello {}".to_string()));
        db.set_file_language(file, Language::Java);
        let text = db.file_text(file);

        assert_eq!(text.as_ref(), "public class Hello {}");
        assert_eq!(db.try_file_text(file).as_deref(), Some(text.as_ref()));
        assert!(db.try_file_text(FileId::new(99)).is_none());
    }

    #[test]
    fn registered_text_revisions_are_persistent_and_snapshot_visible() {
        let mut db = Database::new();
        let file = FileId::new(1);
        db.register_file(
            file,
            Arc::new("class BeforeSnapshot {}".to_string()),
            Language::Java,
        );

        let registered_snapshot = db.registered_file_text.clone();
        db.set_file_text_tracked(file, Arc::new("class AfterSnapshot {}".to_string()));

        assert_eq!(
            registered_snapshot
                .get(&file)
                .map(Arc::as_ref)
                .map(String::as_str),
            Some("class BeforeSnapshot {}")
        );
        let snapshot = db.snapshot();
        assert_eq!(snapshot.file_text(file).as_str(), "class AfterSnapshot {}");
        assert_eq!(
            snapshot.try_file_text(file).as_deref().map(String::as_str),
            Some("class AfterSnapshot {}")
        );
    }

    #[test]
    fn test_parse_file_caching() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Foo {}".to_string()));
        db.set_file_language(file, Language::Java);

        let tree1 = db.parse_file(file);
        let tree2 = db.parse_file(file);

        // Should return the same cached instance
        assert!(Arc::ptr_eq(&tree1, &tree2));
    }

    #[test]
    fn test_parse_file_invalidation() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Foo {}".to_string()));
        db.set_file_language(file, Language::Java);
        let tree1 = db.parse_file(file);

        // Change the file
        db.set_file_text(file, Arc::new("class Bar {}".to_string()));
        let tree2 = db.parse_file(file);

        // Should parse again and return a different instance
        assert!(!Arc::ptr_eq(&tree1, &tree2));
    }

    #[test]
    fn file_semantics_epoch_is_scoped_to_supported_languages() {
        let mut db = Database::new();
        let groovy_file = FileId::new(1);
        let scala_file = FileId::new(2);
        let java_file = FileId::new(3);
        let kotlin_file = FileId::new(4);
        let source = Arc::new("class Example {}".to_string());

        db.register_file(groovy_file, Arc::clone(&source), Language::Groovy);
        db.register_file(scala_file, Arc::clone(&source), Language::Scala);
        db.register_file(java_file, Arc::clone(&source), Language::Java);
        db.register_file(kotlin_file, source, Language::Kotlin);

        let groovy_before = db.file_semantics(groovy_file);
        let scala_before = db.file_semantics(scala_file);
        let java_before = db.file_semantics(java_file);
        let kotlin_before = db.file_semantics(kotlin_file);

        db.bump_workspace_version();

        let groovy_after = db.file_semantics(groovy_file);
        let scala_after = db.file_semantics(scala_file);
        let java_after = db.file_semantics(java_file);
        let kotlin_after = db.file_semantics(kotlin_file);
        assert!(Arc::ptr_eq(&groovy_before, &groovy_after));
        assert!(Arc::ptr_eq(&scala_before, &scala_after));
        assert!(!Arc::ptr_eq(&java_before, &java_after));
        assert!(!Arc::ptr_eq(&kotlin_before, &kotlin_after));

        db.set_file_language(groovy_file, Language::Java);
        let java_semantics = db.file_semantics(groovy_file);
        assert!(!Arc::ptr_eq(&groovy_after, &java_semantics));
    }

    #[test]
    fn scoped_semantics_cache_tracks_index_revision_without_version_notification() {
        use hyperion_workspace::index::SourceScope;

        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("library");
        let client = temp.path().join("client");
        let definition_path = library.join("Box.java");
        let client_path = client.join("Use.java");
        let index = WorkspaceIndex::new(temp.path().to_path_buf());
        index
            .index_file_with_source(
                &definition_path,
                "package api; public class Box { public static int count; }",
            )
            .unwrap();
        let source = "import api.Box; class Use { Object value = Box.count; }";
        index.index_file_with_source(&client_path, source).unwrap();
        let file = index.get_file_id(&client_path).unwrap();
        let scoped = Arc::new(index.with_source_scope(SourceScope::new(vec![library, client])));
        let mut db = Database::new();
        db.set_workspace_index_handle(WorkspaceIndexHandle(Arc::clone(&scoped)));
        db.register_file(file, Arc::new(source.to_owned()), Language::Java);

        let revision = scoped.current_workspace_revision();
        let first = db.file_semantics_at_workspace_revision(file, revision);
        let cached = db.file_semantics_at_workspace_revision(file, revision);
        assert!(Arc::ptr_eq(&first, &cached));
        let start = source.find("Box.count").unwrap() as u32;
        let range = rowan::TextRange::new(start.into(), (start + 9).into());
        assert_eq!(
            first.expression_type_at(range),
            Some(&crate::symbol::Type::Primitive(
                crate::symbol::PrimitiveType::Int
            )),
        );
        index
            .index_file_with_source(
                &definition_path,
                "package api; public class Box { public static boolean count; }",
            )
            .unwrap();
        let refreshed =
            db.file_semantics_at_workspace_revision(file, scoped.current_workspace_revision());
        assert!(!Arc::ptr_eq(&first, &refreshed));
        assert_eq!(
            refreshed.expression_type_at(range),
            Some(&crate::symbol::Type::Primitive(
                crate::symbol::PrimitiveType::Boolean
            )),
        );
        assert_eq!(db.workspace_version(), 0);
    }

    #[test]
    fn tree_sitter_languages_return_a_valid_placeholder_tree() {
        for (id, language, source) in [
            (1, Language::Groovy, "repositories { mavenCentral() }"),
            (2, Language::Scala, "class User"),
        ] {
            let mut db = Database::new();
            let file = FileId::new(id);
            db.set_file_text(file, Arc::new(source.to_string()));
            db.set_file_language(file, language);

            let tree = db.parse_file(file);

            assert!(tree.errors.is_empty(), "{language:?} placeholder errors");
            assert_eq!(tree.syntax().kind(), SyntaxKind::SOURCE_FILE);
            assert!(tree.syntax().children_with_tokens().next().is_none());
        }
    }

    #[test]
    fn test_file_symbols_extraction() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(
            file,
            Arc::new("class MyClass {}\ninterface MyInterface {}".to_string()),
        );
        db.set_file_language(file, Language::Java);

        let symbols = db.file_symbols(file);

        assert_eq!(symbols.symbols.len(), 2);
        assert_eq!(&*symbols.symbols[0].name, "MyClass");
        assert_eq!(symbols.symbols[0].kind, SymbolKind::Class);
        assert_eq!(&*symbols.symbols[1].name, "MyInterface");
        assert_eq!(symbols.symbols[1].kind, SymbolKind::Interface);
    }

    #[test]
    fn test_incremental_computation() {
        let mut db = Database::new();
        let file1 = FileId::new(1);
        let file2 = FileId::new(2);

        db.set_file_text(file1, Arc::new("class A {}".to_string()));
        db.set_file_language(file1, Language::Java);
        db.set_file_text(file2, Arc::new("class B {}".to_string()));
        db.set_file_language(file2, Language::Java);

        let symbols1 = db.file_symbols(file1);
        let symbols2 = db.file_symbols(file2);

        // Modify only file1
        db.set_file_text(file1, Arc::new("class A_Modified {}".to_string()));

        let new_symbols1 = db.file_symbols(file1);
        let new_symbols2 = db.file_symbols(file2);

        // file1 symbols should change
        assert!(!Arc::ptr_eq(&symbols1, &new_symbols1));

        // file2 symbols should be cached (not recomputed)
        assert!(Arc::ptr_eq(&symbols2, &new_symbols2));
    }

    #[test]
    fn test_empty_file() {
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new(String::new()));
        db.set_file_language(file, Language::Java);

        let symbols = db.file_symbols(file);
        assert!(symbols.symbols.is_empty());
    }

    #[test]
    fn test_multiple_files() {
        let mut db = Database::new();

        for i in 0..10 {
            let file = FileId::new(i);
            db.set_file_text(file, Arc::new(format!("class Class{} {{}}", i)));
            db.set_file_language(file, Language::Java);

            let symbols = db.file_symbols(file);
            assert_eq!(symbols.symbols.len(), 1);
            assert_eq!(&*symbols.symbols[0].name, format!("Class{}", i));
        }
    }

    #[test]
    fn test_symbol_table_caching() {
        // Verify that file_symbol_table returns cached Arc (same pointer)
        // This is critical for performance - we shouldn't rebuild the table on every call
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(
            file,
            Arc::new("class Foo { int bar; void baz() {} }".to_string()),
        );
        db.set_file_language(file, Language::Java);

        // First call - builds the table
        let table1 = db.file_symbol_table(file);

        // Second call - should return cached table (same Arc)
        let table2 = db.file_symbol_table(file);

        assert!(
            Arc::ptr_eq(&table1, &table2),
            "file_symbol_table should return cached Arc, not rebuild"
        );

        // Verify the table has the expected symbols
        let stats = table1.statistics();
        assert!(stats.symbol_count > 0, "Table should have symbols");
        assert!(stats.scope_count > 0, "Table should have scopes");
    }

    #[test]
    fn test_symbol_table_invalidation_on_change() {
        // Verify that changing file text invalidates the cached table
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Foo {}".to_string()));
        db.set_file_language(file, Language::Java);

        let table1 = db.file_symbol_table(file);

        // Change the file content
        db.set_file_text(file, Arc::new("class Bar {}".to_string()));

        // Should get a NEW table (different Arc)
        let table2 = db.file_symbol_table(file);

        assert!(
            !Arc::ptr_eq(&table1, &table2),
            "file_symbol_table should rebuild after file change"
        );
    }

    #[test]
    fn test_nullability_analysis_query() {
        // Verify that file_nullability query works and is cached
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(
            file,
            Arc::new(
                r#"
                class Test {
                    void method1() {
                        String s = null;
                        s.length();
                    }

                    void method2() {
                        int x = 5;
                    }
                }
                "#
                .to_string(),
            ),
        );
        db.set_file_language(file, Language::Java);

        // First call - computes analysis
        let analysis1 = db.file_nullability(file);

        // Verify we got results (should have found 2 methods)
        assert_eq!(
            analysis1.methods.len(),
            2,
            "Should analyze both methods in the file"
        );

        // Second call - should return cached result (same Arc)
        let analysis2 = db.file_nullability(file);

        assert!(
            Arc::ptr_eq(&analysis1, &analysis2),
            "file_nullability should return cached Arc"
        );
    }

    #[test]
    fn test_nullability_analysis_invalidation() {
        // Verify that changing file content invalidates nullability analysis
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(file, Arc::new("class Test { void m1() {} }".to_string()));
        db.set_file_language(file, Language::Java);

        let analysis1 = db.file_nullability(file);
        assert_eq!(analysis1.methods.len(), 1);

        // Change the file - add another method
        db.set_file_text(
            file,
            Arc::new("class Test { void m1() {} void m2() {} }".to_string()),
        );

        // Should get NEW analysis (different Arc)
        let analysis2 = db.file_nullability(file);

        assert!(
            !Arc::ptr_eq(&analysis1, &analysis2),
            "file_nullability should rebuild after file change"
        );

        assert_eq!(
            analysis2.methods.len(),
            2,
            "New analysis should find both methods"
        );
    }

    #[test]
    fn test_nullability_non_java_files() {
        // Verify that non-Java files return empty analysis
        let mut db = Database::new();
        let kotlin_file = FileId::new(1);

        db.set_file_text(
            kotlin_file,
            Arc::new("fun test() { val x: String? = null }".to_string()),
        );
        db.set_file_language(kotlin_file, Language::Kotlin);

        let analysis = db.file_nullability(kotlin_file);

        assert!(
            analysis.methods.is_empty(),
            "Kotlin files should not be analyzed (have their own nullability)"
        );
        assert!(
            analysis.diagnostics.is_empty(),
            "No diagnostics for non-Java files"
        );
    }

    #[test]
    fn empty_file_nullability_does_not_depend_on_method_signatures() {
        let mut db = Database::new();
        let file = FileId::new(1);
        db.register_file(
            file,
            Arc::new("class Test { void method() {} }".to_string()),
            Language::Kotlin,
        );

        let kotlin_analysis = db.file_nullability(file);
        assert!(kotlin_analysis.methods.is_empty());

        db.bump_method_signature_version();
        let after_signature_update = db.file_nullability(file);
        assert!(Arc::ptr_eq(&kotlin_analysis, &after_signature_update));

        db.set_file_language(file, Language::Java);
        let java_analysis = db.file_nullability(file);
        assert!(!Arc::ptr_eq(&after_signature_update, &java_analysis));
        assert_eq!(java_analysis.methods.len(), 1);
    }

    #[test]
    fn test_method_signature_version_integration() {
        // Verify that the method signature version input query is integrated with the cache
        let mut db = Database::new();

        // Initial version should be 0
        assert_eq!(db.method_signature_version(), 0);

        // Bump the version via the database method
        db.bump_method_signature_version();

        // Version should now be 1 (synchronized between cache and Salsa)
        assert_eq!(db.method_signature_version(), 1);
        assert_eq!(db.method_signature_cache().version(), 1);

        // Bump again
        db.bump_method_signature_version();

        assert_eq!(db.method_signature_version(), 2);
        assert_eq!(db.method_signature_cache().version(), 2);
    }

    #[test]
    fn test_nullability_invalidation_on_version_bump() {
        // Verify that bumping the signature version invalidates file_nullability queries
        let mut db = Database::new();
        let file = FileId::new(1);

        db.set_file_text(
            file,
            Arc::new("class Test { void method() {} }".to_string()),
        );
        db.set_file_language(file, Language::Java);

        // First query - computes nullability
        let analysis1 = db.file_nullability(file);

        // Second query - should return cached (same Arc)
        let analysis2 = db.file_nullability(file);
        assert!(Arc::ptr_eq(&analysis1, &analysis2));

        // Bump the method signature version (simulating a signature change in another file)
        db.bump_method_signature_version();

        // Third query - should recompute because version changed (different Arc)
        let analysis3 = db.file_nullability(file);

        // The analysis should have been recomputed, so different Arc
        assert!(
            !Arc::ptr_eq(&analysis1, &analysis3),
            "file_nullability should be invalidated when method signature version changes"
        );

        // But the fourth query should be cached again
        let analysis4 = db.file_nullability(file);
        assert!(Arc::ptr_eq(&analysis3, &analysis4));
    }

    #[test]
    fn cross_file_method_contracts_affect_nullability_results() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let client = FileId::new(2);

        db.register_file(
            provider,
            Arc::new(
                r#"
                package api;
                class Service {
                    @Nullable String load() { return null; }
                    void consume(@Nonnull String value) {}
                    void permissive(String value) {}
                }
                "#
                .to_string(),
            ),
            Language::Java,
        );
        let provider_tree = db.parse_file(provider);
        let signatures =
            crate::nullability::extract_method_signatures(&provider_tree.syntax(), provider);
        assert!(db
            .method_signature_cache()
            .update_file_signatures(provider, signatures));
        db.bump_method_signature_version();

        db.register_file(
            client,
            Arc::new(
                r#"
                package app;
                import api.Service;
                class Client {
                    void run(Service service) {
                        String loaded = service.load();
                        loaded.length();
                        String missing = null;
                        service.consume(missing);
                        service.permissive(missing);
                    }
                }
                "#
                .to_string(),
            ),
            Language::Java,
        );

        let analysis = db.file_nullability(client);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "loaded"
        }));
        assert_eq!(
            analysis
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.kind == crate::nullability::DiagnosticKind::NullPassedToNonNull
                        && diagnostic.variable_name == "missing"
                })
                .count(),
            1,
            "an unannotated parameter is not a cross-file non-null contract"
        );

        let nullable_snapshot = db.snapshot();
        let nonnull_provider = Arc::new(
            r#"
                package api;
                class Service {
                    @Nonnull String load() { return "ready"; }
                    void consume(@Nonnull String value) {}
                    void permissive(String value) {}
                }
                "#
            .to_string(),
        );
        let tokens = Lexer::new(&nonnull_provider).collect();
        let (green, _) = Parser::parse(tokens);
        let signatures =
            crate::nullability::extract_method_signatures(&SyntaxNode::new_root(green), provider);
        assert!(nullable_snapshot
            .method_signature_cache()
            .update_file_signatures(provider, signatures));

        assert!(nullable_snapshot
            .file_nullability(client)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.variable_name == "loaded"));
        drop(nullable_snapshot);

        db.set_file_text_tracked(provider, nonnull_provider);
        db.bump_method_signature_version();
        assert!(!db
            .file_nullability(client)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.variable_name == "loaded"));
    }

    #[test]
    fn cross_file_owner_resolution_respects_package_and_wildcard_ambiguity() {
        let cache = crate::nullability::MethodSignatureCache::new();
        for (file_id, fqn) in [
            (FileId::new(1), "api.Service.load()"),
            (FileId::new(2), "other.Service.load()"),
        ] {
            assert!(cache.update_file_signatures(
                file_id,
                vec![(
                    fqn.to_string(),
                    crate::nullability::MethodSignature::new(
                        crate::nullability::Nullability::Nullable,
                        Vec::new(),
                        file_id,
                    ),
                )],
            ));
        }
        let signatures = cache.snapshot();
        let parse = |source: &str| {
            let tokens = Lexer::new(source).collect();
            let (green, _) = Parser::parse(tokens);
            SyntaxNode::new_root(green)
        };

        let same_package = JavaMethodOwnerResolver::from_root(&parse("package api; class C {}"));
        assert_eq!(
            same_package.resolve_call_owner("Service", "load", 0, &signatures),
            "api.Service"
        );

        let one_wildcard =
            JavaMethodOwnerResolver::from_root(&parse("package app; import api.*; class C {}"));
        assert_eq!(
            one_wildcard.resolve_call_owner("Service", "load", 0, &signatures),
            "api.Service"
        );

        let ambiguous = JavaMethodOwnerResolver::from_root(&parse(
            "package app; import api.*; import other.*; class C {}",
        ));
        assert_eq!(
            ambiguous.resolve_call_owner("Service", "load", 0, &signatures),
            "Service",
            "ambiguous wildcard imports must not borrow either package's contract"
        );
    }

    #[test]
    fn current_package_type_precedes_default_and_on_demand_member_candidates() {
        let cache = crate::nullability::MethodSignatureCache::new();
        let default_root = parse_java_for_member_facts(
            "class Values { public static int TARGET = 1; static void load() {} }",
        );
        let current_root = parse_java_for_member_facts(
            "package app; class Values { public static int CURRENT = 2; }",
        );
        let wildcard_root = parse_java_for_member_facts(
            "package imported; class Values { public static int TARGET = 3; static void load() {} }",
        );
        publish_member_facts(&cache, FileId::new(1), &default_root);
        publish_member_facts(&cache, FileId::new(2), &current_root);
        publish_member_facts(&cache, FileId::new(3), &wildcard_root);
        let snapshot = cache.snapshot();
        let caller_root =
            parse_java_for_member_facts("package app; import imported.*; class Client {}");
        let resolver = JavaMethodOwnerResolver::from_root(&caller_root);

        assert_eq!(
            resolver.resolve_call_owner("Values", "load", 0, &snapshot),
            "app.Values",
            "a current-package type wins even when only the wildcard type has the method"
        );
        assert!(
            resolver
                .resolve_field("Values", "TARGET", "Client", &snapshot)
                .is_none(),
            "a missing member on the selected current-package type must not fall through"
        );
        assert_eq!(
            resolver
                .resolve_field("Values", "CURRENT", "Client", &snapshot)
                .map(|(owner, _)| owner.to_string()),
            Some("app.Values".to_owned())
        );
    }

    #[test]
    fn nested_type_method_summaries_do_not_collide_by_simple_name() {
        let mut db = Database::new();
        let file = FileId::new(1);
        db.register_file(
            file,
            Arc::new(
                r#"
                class Outer {
                    class Inner {
                        @Nullable String value() { return null; }
                        void check() {
                            String outerValue = value();
                            outerValue.length();
                        }
                    }
                }
                class Other {
                    class Inner {
                        @Nonnull String value() { return "ok"; }
                        void check() {
                            String otherValue = value();
                            otherValue.length();
                        }
                    }
                }
                "#
                .to_string(),
            ),
            Language::Java,
        );

        let analysis = db.file_nullability(file);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "outerValue"
        }));
        assert!(!analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.variable_name == "otherValue"));
    }

    #[test]
    fn cross_file_contracts_cover_all_named_type_kinds_and_nested_owners() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let client = FileId::new(2);
        db.register_file(
            provider,
            Arc::new(
                r#"
                package api;
                interface Gateway { @Nullable String fetch(); }
                abstract class Base { @Nullable abstract String read(); }
                enum Mode { FAST; @Nullable String label() { return null; } }
                record Result(String raw) {
                    @Nullable String display() { return null; }
                }
                @interface Marker { @Nullable String value(); }
                class Outer {
                    static class Inner {
                        @Nullable String nested() { return null; }
                    }
                }
                "#
                .to_string(),
            ),
            Language::Java,
        );
        let provider_tree = db.parse_file(provider);
        let signatures =
            crate::nullability::extract_method_signatures(&provider_tree.syntax(), provider);
        assert!(db
            .method_signature_cache()
            .update_file_signatures(provider, signatures));
        db.bump_method_signature_version();

        db.register_file(
            client,
            Arc::new(
                r#"
                package app;
                import api.*;
                import api.Outer.Inner;
                class Client {
                    void run(
                        Gateway gateway,
                        Base base,
                        Mode mode,
                        Result result,
                        Marker marker,
                        Inner inner
                    ) {
                        String gatewayValue = gateway.fetch();
                        gatewayValue.length();
                        String baseValue = base.read();
                        baseValue.length();
                        String enumValue = mode.label();
                        enumValue.length();
                        String recordValue = result.display();
                        recordValue.length();
                        String annotationValue = marker.value();
                        annotationValue.length();
                        String nestedValue = inner.nested();
                        nestedValue.length();
                    }
                }
                "#
                .to_string(),
            ),
            Language::Java,
        );

        let analysis = db.file_nullability(client);
        let nullable_variables: std::collections::HashSet<_> = analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
            })
            .map(|diagnostic| diagnostic.variable_name.as_str())
            .collect();
        for expected in [
            "gatewayValue",
            "baseValue",
            "enumValue",
            "recordValue",
            "annotationValue",
            "nestedValue",
        ] {
            assert!(
                nullable_variables.contains(expected),
                "missing {expected} contract; diagnostics: {:?}",
                analysis.diagnostics
            );
        }
    }

    #[test]
    fn nullable_call_results_are_checked_as_direct_dereference_receivers() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let client = FileId::new(2);
        db.register_file(
            provider,
            Arc::new(
                "package api; class Service { @Nullable String load() { return null; } }"
                    .to_string(),
            ),
            Language::Java,
        );
        let provider_tree = db.parse_file(provider);
        let signatures =
            crate::nullability::extract_method_signatures(&provider_tree.syntax(), provider);
        assert!(db
            .method_signature_cache()
            .update_file_signatures(provider, signatures));
        db.bump_method_signature_version();

        db.register_file(
            client,
            Arc::new(
                concat!(
                    "package app; import api.Service; class Client { ",
                    "void run(Service service) { service.load().length(); } }"
                )
                .to_string(),
            ),
            Language::Java,
        );

        let analysis = db.file_nullability(client);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "load()"
                && diagnostic.nullability == crate::nullability::Nullability::Nullable
        }));
    }

    #[test]
    fn indexed_declaration_owner_overrides_the_lexical_receiver_type() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let implementation = FileId::new(2);
        let client = FileId::new(3);
        let source = concat!(
            "package app; import impl.Implementation; class Client { ",
            "void run(Implementation implementation) { implementation.load().length(); } }"
        );
        db.register_file(client, Arc::new(source.to_string()), Language::Java);
        assert!(db.method_signature_cache().update_indexed_call_bindings(
            client,
            11,
            vec![
                (
                    u32::try_from(source.find("load").expect("load offset")).expect("u32 offset"),
                    Arc::from("api.Contract.load()"),
                ),
                (
                    u32::try_from(source.find("length").expect("length offset"))
                        .expect("u32 offset"),
                    Arc::from("impl.Implementation.length()"),
                ),
            ],
        ));
        db.bump_method_signature_version();
        assert!(db.method_signature_cache().update_indexed_file_signatures(
            provider,
            22,
            vec![(
                "api.Contract.load()".to_string(),
                crate::nullability::MethodSignature::new(
                    crate::nullability::Nullability::Nullable,
                    Vec::new(),
                    provider,
                ),
            )],
        ));
        db.bump_method_signature_version();
        assert!(db.method_signature_cache().update_indexed_file_signatures(
            implementation,
            33,
            Vec::new()
        ));
        db.bump_method_signature_version();

        let analysis = db.file_nullability(client);
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                && diagnostic.variable_name == "load()"
        }));
    }

    #[test]
    fn unknown_and_nonnull_call_results_do_not_create_chain_diagnostics() {
        let mut db = Database::new();
        let file = FileId::new(1);
        db.register_file(
            file,
            Arc::new(
                r#"
                abstract class SafeChains {
                    abstract String platformValue();
                    @Nonnull String knownValue() { return "ok"; }
                    void run() {
                        platformValue().length();
                        knownValue().length();
                        new String().length();
                    }
                }
                "#
                .to_string(),
            ),
            Language::Java,
        );

        assert!(db.file_nullability(file).diagnostics.is_empty());
    }

    fn parse_java_for_member_facts(source: &str) -> SyntaxNode {
        let (green, errors) = Parser::parse(Lexer::new(source).collect());
        assert!(errors.is_empty(), "fixture parse errors: {errors:?}");
        SyntaxNode::new_root(green)
    }

    fn publish_member_facts(
        cache: &crate::nullability::MethodSignatureCache,
        file: FileId,
        root: &SyntaxNode,
    ) {
        assert!(cache.update_file_member_facts(
            file,
            crate::nullability::extract_method_signatures(root, file),
            crate::nullability::extract_field_signatures(root, file),
            crate::nullability::extract_declared_type_names(root),
            crate::nullability::extract_type_hierarchy_facts(root),
        ));
    }

    fn method_named(root: &SyntaxNode, name: &str) -> SyntaxNode {
        root.descendants()
            .find(|node| {
                node.kind() == SyntaxKind::METHOD_DECL
                    && crate::nullability::extract_method_info(node)
                        .is_some_and(|info| info.name == name)
            })
            .unwrap_or_else(|| panic!("missing method {name}"))
    }

    fn lexical_resolver(scope: &SyntaxNode) -> crate::nullability::NullabilityResolver {
        let mut resolver = crate::nullability::NullabilityResolver::new();
        resolver.register_scoped_body_bindings(scope);
        resolver
    }

    #[test]
    fn indexed_dependency_call_preserves_selected_owner_conversions_return_and_effect() {
        use crate::nullability::analysis::{DispatchKind, InvocationEffect, QualifierKind};
        use crate::nullability::patterns::ValueKind;

        let provider = FileId::new(1);
        let caller = FileId::new(2);
        let provider_root = parse_java_for_member_facts(
            r#"
            package api;
            class Base {
                static double convert(int first, Object second) { return 2.0; }
            }
            "#,
        );
        let caller_source = r#"
            package app;
            import impl.Child;
            class Client {
                void run(Integer first, Object second) {
                    Child.convert(first, second);
                }
            }
        "#;
        let caller_root = parse_java_for_member_facts(caller_source);
        let scope = method_named(&caller_root, "run");
        let offset = u32::try_from(caller_source.find("convert").expect("call offset"))
            .expect("offset fits u32");
        let cache = crate::nullability::MethodSignatureCache::new();
        publish_member_facts(&cache, provider, &provider_root);
        assert!(cache.update_indexed_call_bindings(
            caller,
            11,
            vec![(offset, Arc::from("api.Base.convert(int,Object)"))],
        ));
        let snapshot = cache.snapshot();
        let resolver = lexical_resolver(&scope);
        let (uses, conversions) = resolve_source_member_uses(
            caller,
            &scope,
            &resolver,
            &HashMap::new(),
            &HashMap::new(),
            &crate::nullability::source_methods::SourceMethodTable::default(),
            &HashMap::new(),
            &HashMap::new(),
            &JavaMethodOwnerResolver::from_root(&caller_root),
            &snapshot,
        );

        let use_fact = uses.get(&offset).expect("indexed call fact");
        assert_eq!(use_fact.dispatch(), DispatchKind::Static);
        assert_eq!(use_fact.qualifier(), QualifierKind::TypeName);
        assert_eq!(use_fact.value_kind, ValueKind::PrimitiveFloating);
        assert_eq!(
            use_fact.effect(),
            InvocationEffect::ClassInitialization,
            "a locally non-throwing static source call has only class-initialization effects"
        );
        let plan = usize::from(
            use_fact
                .argument_plan
                .checked_sub(1)
                .expect("conversion plan"),
        );
        assert_eq!(
            conversions[plan].as_ref(),
            [ValueKind::PrimitiveNumeric, ValueKind::Reference]
        );
    }

    #[test]
    fn indexed_external_binding_without_published_declaration_stays_conservative() {
        let caller = FileId::new(1);
        let source = concat!(
            "package app; class Client { void run(Integer value) { ",
            "external.Dependency.accept(value); } }",
        );
        let root = parse_java_for_member_facts(source);
        let scope = method_named(&root, "run");
        let offset =
            u32::try_from(source.find("accept").expect("call offset")).expect("offset fits u32");
        let cache = crate::nullability::MethodSignatureCache::new();
        assert!(cache.update_indexed_call_bindings(
            caller,
            11,
            vec![(offset, Arc::from("external.Dependency.accept(int)"))],
        ));
        let snapshot = cache.snapshot();
        let resolver = lexical_resolver(&scope);
        let (uses, conversions) = resolve_source_member_uses(
            caller,
            &scope,
            &resolver,
            &HashMap::new(),
            &HashMap::new(),
            &crate::nullability::source_methods::SourceMethodTable::default(),
            &HashMap::new(),
            &HashMap::new(),
            &JavaMethodOwnerResolver::from_root(&root),
            &snapshot,
        );

        assert!(!uses.contains_key(&offset));
        assert!(conversions.is_empty());
        assert!(snapshot.indexed_call_is_static(caller, offset).is_none());
        assert!(snapshot.indexed_call_value_kind(caller, offset).is_none());
        assert!(snapshot.indexed_call_may_throw(caller, offset).is_none());
    }

    #[test]
    fn uppercase_variable_qualifier_is_not_mistaken_for_a_type_name() {
        use crate::nullability::analysis::QualifierKind;

        let provider = FileId::new(1);
        let caller = FileId::new(2);
        let provider_root = parse_java_for_member_facts(
            "package api; class Base { public static int value() { return 1; } }",
        );
        let source = concat!(
            "package app; import impl.Child; class Client { void run(Child Child) { ",
            "int value = Child.value(); } }",
        );
        let root = parse_java_for_member_facts(source);
        let scope = method_named(&root, "run");
        let offset =
            u32::try_from(source.rfind("value").expect("call offset")).expect("offset fits u32");
        let cache = crate::nullability::MethodSignatureCache::new();
        publish_member_facts(&cache, provider, &provider_root);
        assert!(cache.update_indexed_call_bindings(
            caller,
            11,
            vec![(offset, Arc::from("api.Base.value()"))],
        ));
        let snapshot = cache.snapshot();
        let resolver = lexical_resolver(&scope);
        let (uses, _) = resolve_source_member_uses(
            caller,
            &scope,
            &resolver,
            &HashMap::new(),
            &HashMap::new(),
            &crate::nullability::source_methods::SourceMethodTable::default(),
            &HashMap::new(),
            &HashMap::new(),
            &JavaMethodOwnerResolver::from_root(&root),
            &snapshot,
        );

        assert_eq!(
            uses.get(&offset).expect("indexed call").qualifier(),
            QualifierKind::Value
        );
    }

    #[test]
    fn dependency_field_facts_preserve_dispatch_qualifier_and_value_kind() {
        use crate::nullability::analysis::{DispatchKind, QualifierKind};
        use crate::nullability::patterns::ValueKind;

        let provider = FileId::new(1);
        let caller = FileId::new(2);
        let provider_root = parse_java_for_member_facts(
            r#"
            package api;
            class Values {
                public static double DIVISOR = 2.0;
                public double ratio = 2.0;
            }
            "#,
        );
        let caller_source = r#"
            package app;
            import api.Values;
            class Client {
                void run(Values values) {
                    double a = values.DIVISOR;
                    double b = values.ratio;
                    double c = Values.DIVISOR;
                }
            }
        "#;
        let caller_root = parse_java_for_member_facts(caller_source);
        let scope = method_named(&caller_root, "run");
        let first_divisor = u32::try_from(caller_source.find("DIVISOR").expect("first field"))
            .expect("offset fits u32");
        let ratio = u32::try_from(caller_source.find("ratio").expect("instance field"))
            .expect("offset fits u32");
        let type_divisor = u32::try_from(caller_source.rfind("DIVISOR").expect("second field"))
            .expect("offset fits u32");
        let cache = crate::nullability::MethodSignatureCache::new();
        publish_member_facts(&cache, provider, &provider_root);
        let snapshot = cache.snapshot();
        let resolver = lexical_resolver(&scope);
        let (uses, _) = resolve_source_member_uses(
            caller,
            &scope,
            &resolver,
            &HashMap::new(),
            &HashMap::new(),
            &crate::nullability::source_methods::SourceMethodTable::default(),
            &HashMap::new(),
            &HashMap::new(),
            &JavaMethodOwnerResolver::from_root(&caller_root),
            &snapshot,
        );

        let value_static = uses
            .get(&first_divisor)
            .expect("value-qualified static field");
        assert_eq!(value_static.dispatch(), DispatchKind::Static);
        assert_eq!(value_static.qualifier(), QualifierKind::Value);
        assert_eq!(value_static.value_kind, ValueKind::PrimitiveFloating);

        let instance = uses.get(&ratio).expect("instance field");
        assert_eq!(instance.dispatch(), DispatchKind::Instance);
        assert_eq!(instance.qualifier(), QualifierKind::Value);
        assert_eq!(instance.value_kind, ValueKind::PrimitiveFloating);

        let type_static = uses
            .get(&type_divisor)
            .expect("type-qualified static field");
        assert_eq!(type_static.dispatch(), DispatchKind::Static);
        assert_eq!(type_static.qualifier(), QualifierKind::TypeName);
        assert_eq!(type_static.value_kind, ValueKind::PrimitiveFloating);
    }

    #[test]
    fn dependency_field_nullability_flows_through_static_and_instance_access() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        let provider_source = r#"
            package api;
            class Values {
                @Nullable public static String OPTIONAL;
                @Nullable public String instanceOptional;
            }
        "#;
        db.register_file(
            provider,
            Arc::new(provider_source.to_owned()),
            Language::Java,
        );
        let provider_root = db.parse_file(provider).syntax();
        publish_member_facts(db.method_signature_cache(), provider, &provider_root);

        let caller_source = r#"
            package app;
            import api.Values;
            class Client {
                void run(Values values) {
                    Values.OPTIONAL.toString();
                    values.OPTIONAL.toString();
                    values.instanceOptional.toString();
                }
            }
        "#;
        db.register_file(caller, Arc::new(caller_source.to_owned()), Language::Java);
        db.bump_method_signature_version();

        let analysis = db.file_nullability(caller);
        assert_eq!(
            analysis
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                })
                .count(),
            3,
            "every nullable field value is dereferenced, while static qualifiers themselves are not: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn bare_inherited_dependency_field_uses_the_canonical_field_contract() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let child = FileId::new(2);
        let provider_source = r#"
            package api;
            class Base { @Nullable protected String value; }
        "#;
        db.register_file(
            provider,
            Arc::new(provider_source.to_owned()),
            Language::Java,
        );
        let provider_root = db.parse_file(provider).syntax();
        publish_member_facts(db.method_signature_cache(), provider, &provider_root);

        let child_source = r#"
            package api;
            class Child extends Base {
                void run() { value.toString(); }
            }
        "#;
        db.register_file(child, Arc::new(child_source.to_owned()), Language::Java);
        let child_root = db.parse_file(child).syntax();
        publish_member_facts(db.method_signature_cache(), child, &child_root);
        db.bump_method_signature_version();

        let snapshot = db.method_signature_cache().snapshot();
        assert_eq!(
            snapshot
                .resolve_field_signature_from("api.Child", "value", Some("api"), "api.Child",)
                .map(|(_, field)| field.nullability()),
            Some(crate::nullability::Nullability::Nullable),
            "published hierarchy must resolve the accessible inherited field"
        );

        let scope = method_named(&child_root, "run");
        let offset = u32::try_from(
            child_source
                .find("value.toString")
                .expect("bare inherited field offset"),
        )
        .expect("offset fits u32");
        let resolver = lexical_resolver(&scope);
        let (uses, _) = resolve_source_member_uses(
            child,
            &scope,
            &resolver,
            &HashMap::new(),
            &HashMap::new(),
            &crate::nullability::source_methods::SourceMethodTable::default(),
            &HashMap::new(),
            &HashMap::new(),
            &JavaMethodOwnerResolver::from_root(&child_root),
            &snapshot,
        );
        assert_eq!(
            uses.get(&offset).map(|member| member.result_nullability()),
            Some(crate::nullability::Nullability::Nullable),
            "bare inherited field must bind to the canonical field fact: {uses:?}"
        );

        let analysis = db.file_nullability(child);
        assert!(
            analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                    && diagnostic.variable_name == "value"
            }),
            "bare and this-qualified inherited fields must share one contract: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn scoped_bindings_win_over_a_same_named_inherited_field() {
        let provider = FileId::new(1);
        let child = FileId::new(2);
        let provider_root = parse_java_for_member_facts(
            "package api; class Base { @Nullable protected String value; }",
        );
        let child_source = r#"
            package api;
            class Child extends Base {
                void run(Object candidate) throws Exception {
                    if (candidate instanceof String value && !value.isEmpty()) {
                        value.length();
                    }
                    switch (candidate) {
                        case String value -> value.length();
                        default -> {}
                    }
                    try (java.io.StringReader value = new java.io.StringReader("")) {
                        value.read();
                    }
                }
            }
        "#;
        let child_root = parse_java_for_member_facts(child_source);
        let scope = method_named(&child_root, "run");
        let cache = crate::nullability::MethodSignatureCache::new();
        publish_member_facts(&cache, provider, &provider_root);
        publish_member_facts(&cache, child, &child_root);
        let snapshot = cache.snapshot();
        let resolver = lexical_resolver(&scope);
        let (uses, _) = resolve_source_member_uses(
            child,
            &scope,
            &resolver,
            &HashMap::new(),
            &HashMap::new(),
            &crate::nullability::source_methods::SourceMethodTable::default(),
            &HashMap::new(),
            &HashMap::new(),
            &JavaMethodOwnerResolver::from_root(&child_root),
            &snapshot,
        );

        for spelling in ["value.isEmpty", "value.length", "value.read"] {
            for (offset, _) in child_source.match_indices(spelling) {
                let offset = u32::try_from(offset).expect("offset fits u32");
                assert!(
                    !uses.contains_key(&offset),
                    "the resolver's scoped binding for {spelling} must win over Base.value: {uses:?}"
                );
            }
        }
    }

    #[test]
    fn super_field_bypasses_a_shadowing_current_class_field() {
        let mut db = Database::new();
        let file = FileId::new(1);
        let source = r#"
            package api;
            class Base { @Nullable protected String value; }
            class Child extends Base {
                @Nonnull String value = "safe";
                void run() { super.value.toString(); }
            }
        "#;
        db.register_file(file, Arc::new(source.to_owned()), Language::Java);
        let root = db.parse_file(file).syntax();
        publish_member_facts(db.method_signature_cache(), file, &root);
        db.bump_method_signature_version();

        let analysis = db.file_nullability(file);
        assert!(
            analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                    && diagnostic.variable_name == "super.value"
            }),
            "super.value must use Base.value's nullable contract, not Child.value: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn inherited_dependency_field_without_hierarchy_fact_stays_unresolved() {
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        let provider_root = parse_java_for_member_facts(
            "package base; class Base { public static int VALUE = 1; }",
        );
        let caller_source = r#"
            package app;
            import impl.Child;
            class Client { void run() { int value = Child.VALUE; } }
        "#;
        let caller_root = parse_java_for_member_facts(caller_source);
        let scope = method_named(&caller_root, "run");
        let offset = u32::try_from(caller_source.find("VALUE").expect("field offset"))
            .expect("offset fits u32");
        let cache = crate::nullability::MethodSignatureCache::new();
        publish_member_facts(&cache, provider, &provider_root);
        let snapshot = cache.snapshot();
        assert!(snapshot.field_signature("impl.Child", "VALUE").is_none());
        let resolver = lexical_resolver(&scope);
        let (uses, _) = resolve_source_member_uses(
            caller,
            &scope,
            &resolver,
            &HashMap::new(),
            &HashMap::new(),
            &crate::nullability::source_methods::SourceMethodTable::default(),
            &HashMap::new(),
            &HashMap::new(),
            &JavaMethodOwnerResolver::from_root(&caller_root),
            &snapshot,
        );

        assert!(
            !uses.contains_key(&offset),
            "must not attach Base facts to an unresolved Child field: {uses:?}"
        );
    }

    #[test]
    fn dependency_conversion_plan_unboxes_before_evaluating_later_arguments() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        let provider_source = r#"
            package api;
            class Base { static void accept(int first, Object second) {} }
        "#;
        db.register_file(
            provider,
            Arc::new(provider_source.to_owned()),
            Language::Java,
        );
        let provider_root = db.parse_file(provider).syntax();
        publish_member_facts(db.method_signature_cache(), provider, &provider_root);

        let caller_source = r#"
            package app;
            import impl.Child;
            class Client {
                void run(@Nullable Integer first) {
                    Object marker = null;
                    try {
                        Child.accept(first, marker = new Object());
                    } catch (NullPointerException ex) {
                        marker.toString();
                    }
                }
            }
        "#;
        db.register_file(caller, Arc::new(caller_source.to_owned()), Language::Java);
        let offset = u32::try_from(caller_source.find("accept").expect("call offset"))
            .expect("offset fits u32");
        assert!(db.method_signature_cache().update_indexed_call_bindings(
            caller,
            11,
            vec![(offset, Arc::from("api.Base.accept(int,Object)"))],
        ));
        db.bump_method_signature_version();

        let analysis = db.file_nullability(caller);
        assert!(
            analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                    && diagnostic.variable_name == "marker"
            }),
            "the first argument's unboxing NPE must skip the later assignment: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn dependency_conversions_cover_constructor_initializer_and_field_executables() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        let provider_source =
            "package api; class Dependency { static int take(int value) { return value; } }";
        db.register_file(
            provider,
            Arc::new(provider_source.to_owned()),
            Language::Java,
        );
        let provider_root = db.parse_file(provider).syntax();
        publish_member_facts(db.method_signature_cache(), provider, &provider_root);

        let caller_source = r#"
            package app;
            import api.Dependency;
            class Client {
                @Nullable Integer source;
                int value = Dependency.take(source);
                { Dependency.take(source); }
                Client() { Dependency.take(source); }
            }
        "#;
        db.register_file(caller, Arc::new(caller_source.to_owned()), Language::Java);
        let bindings: Vec<_> = caller_source
            .match_indices("take")
            .map(|(offset, _)| {
                (
                    u32::try_from(offset).expect("offset fits u32"),
                    Arc::from("api.Dependency.take(int)"),
                )
            })
            .collect();
        assert_eq!(bindings.len(), 3);
        assert!(db
            .method_signature_cache()
            .update_indexed_call_bindings(caller, 11, bindings));
        db.bump_method_signature_version();

        let analysis = db.file_nullability(caller);
        assert!(
            analysis.methods.len() >= 3,
            "every executable context must retain its analysis: {:?}",
            analysis.methods.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            analysis
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                        && diagnostic.variable_name == "source"
                })
                .count(),
            3,
            "field, initializer, and constructor calls must all apply the indexed conversion: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn static_field_active_use_preserves_class_initialization_exception_flow() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        let provider_source =
            "package api; class Values { public static int COUNT = initialize(); }";
        db.register_file(
            provider,
            Arc::new(provider_source.to_owned()),
            Language::Java,
        );
        let provider_root = db.parse_file(provider).syntax();
        publish_member_facts(db.method_signature_cache(), provider, &provider_root);

        let caller_source = r#"
            package app;
            import api.Values;
            class Client {
                void typeRead() {
                    @Nullable Object typeMarker = null;
                    try {
                        int ignored = Values.COUNT;
                        typeMarker = new Object();
                    } finally {
                        typeMarker.toString();
                    }
                }
                void valueRead(Values values) {
                    @Nullable Object valueMarker = null;
                    try {
                        int ignored = values.COUNT;
                        valueMarker = new Object();
                    } catch (Throwable failure) {
                        valueMarker.toString();
                    }
                }
                void typeWrite() {
                    @Nullable Object writeMarker = null;
                    try {
                        Values.COUNT = 1;
                        writeMarker = new Object();
                    } catch (Throwable failure) {
                        writeMarker.toString();
                    }
                }
            }
        "#;
        db.register_file(caller, Arc::new(caller_source.to_owned()), Language::Java);
        db.bump_method_signature_version();

        let analysis = db.file_nullability(caller);
        for marker in ["typeMarker", "valueMarker", "writeMarker"] {
            assert!(
                analysis.diagnostics.iter().any(|diagnostic| {
                    diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                        && diagnostic.variable_name == marker
                }),
                "missing class-initialization exceptional flow for {marker}: {:?}",
                analysis.diagnostics
            );
        }
    }

    #[test]
    fn class_initialization_effect_is_error_bounded_not_arbitrary() {
        let mut db = Database::new();
        let provider = FileId::new(1);
        let caller = FileId::new(2);
        let provider_source = concat!(
            "package api; class Values { public static int COUNT = 1; ",
            "public static int safe() { return 1; } }",
        );
        db.register_file(
            provider,
            Arc::new(provider_source.to_owned()),
            Language::Java,
        );
        let provider_root = db.parse_file(provider).syntax();
        publish_member_facts(db.method_signature_cache(), provider, &provider_root);

        let caller_source = r#"
            package app;
            import api.Values;
            class Client {
                void run() {
                    @Nullable Object fieldMarker = null;
                    try {
                        int ignored = Values.COUNT;
                        fieldMarker = new Object();
                    } catch (Exception ex) {
                        fieldMarker.toString();
                    }
                    @Nullable Object methodMarker = null;
                    try {
                        int ignored = Values.safe();
                        methodMarker = new Object();
                    } catch (Exception ex) {
                        methodMarker.toString();
                    }
                    @Nullable Object errorMarker = null;
                    try {
                        int ignored = Values.COUNT;
                        errorMarker = new Object();
                    } catch (Error failure) {
                        errorMarker.toString();
                    }
                }
            }
        "#;
        db.register_file(caller, Arc::new(caller_source.to_owned()), Language::Java);
        db.bump_method_signature_version();

        let analysis = db.file_nullability(caller);
        for impossible_exception_marker in ["fieldMarker", "methodMarker"] {
            assert!(
                !analysis.diagnostics.iter().any(|diagnostic| {
                    diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                        && diagnostic.variable_name == impossible_exception_marker
                }),
                "class initialization does not route to catch (Exception): {:?}",
                analysis.diagnostics
            );
        }
        assert!(
            analysis.diagnostics.iter().any(|diagnostic| {
                diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                    && diagnostic.variable_name == "errorMarker"
            }),
            "class initialization failure must retain the Error lane: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn initialization_and_constructor_diagnostics_never_see_later_guarantees() {
        let mut db = Database::new();
        let file = FileId::new(1);
        let source = r#"
            class Client {
                static String staticEarly = Client.staticLate.toString();
                static { Client.staticBlockLate.toString(); }
                static String staticLate = "ready";
                static String staticBlockLate = "ready";

                String instanceEarly = this.instanceLate.toString();
                { this.instanceBlockLate.toString(); }
                String bareEarly = instanceBareLate.toString();
                String instanceLate = "ready";
                String instanceBlockLate = "ready";
                String instanceBareLate = "ready";

                String constructorLate;
                Client() {
                    this.constructorLate.toString();
                    this.constructorLate = "ready";
                }
                void afterConstruction() {
                    this.constructorLate.toString();
                    this.instanceLate.toString();
                }
            }
        "#;
        db.register_file(file, Arc::new(source.to_owned()), Language::Java);

        let analysis = db.file_nullability(file);
        let null_dereferences: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
            })
            .collect();
        for field in [
            "staticLate",
            "staticBlockLate",
            "instanceLate",
            "instanceBlockLate",
            "instanceBareLate",
            "constructorLate",
        ] {
            assert!(
                null_dereferences
                    .iter()
                    .any(|diagnostic| diagnostic.variable_name.ends_with(field)),
                "the earlier use of {field} must observe Java's default null value: {:?}",
                analysis.diagnostics
            );
        }
        assert_eq!(
            null_dereferences
                .iter()
                .filter(|diagnostic| diagnostic.variable_name.ends_with("constructorLate"))
                .count(),
            1,
            "the ordinary method may use the post-construction intersection: {:?}",
            analysis.diagnostics
        );
    }

    #[test]
    fn same_arity_constructor_overloads_never_cross_contaminate_delegation() {
        let mut db = Database::new();
        let file = FileId::new(1);
        let source = r#"
            class Client {
                String value;
                Client(String selected) { this.value = "ready"; }
                Client(Object selected) {}
                Client() {
                    this((Object) null);
                    this.value.toString();
                }
                Client(int exactString) {
                    this("selected");
                    this.value.toString();
                }
                Client(boolean unknown, Object selected) {
                    this(unknown ? "selected" : selected);
                    this.value.toString();
                }
            }
        "#;
        db.register_file(file, Arc::new(source.to_owned()), Language::Java);

        let analysis = db.file_nullability(file);
        let dereferences: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.kind == crate::nullability::DiagnosticKind::PotentialNullDereference
                    && diagnostic.variable_name.ends_with("value")
            })
            .collect();
        assert_eq!(
            dereferences.len(),
            2,
            "the exact String delegation is safe, while the exact Object and unresolved same-arity delegations must not borrow its guarantee: {:?}",
            analysis.diagnostics,
        );
        let exact_object_start = source.find("Client() {").expect("Object constructor");
        let exact_string_start = source
            .find("Client(int exactString)")
            .expect("String constructor");
        let unresolved_start = source
            .find("Client(boolean unknown")
            .expect("unresolved constructor");
        let diagnostics_in = |start: usize, end: usize| {
            dereferences
                .iter()
                .filter(|diagnostic| {
                    let offset = diagnostic.location.start as usize;
                    (start..end).contains(&offset)
                })
                .count()
        };
        assert_eq!(
            (
                diagnostics_in(exact_object_start, exact_string_start),
                diagnostics_in(exact_string_start, unresolved_start),
                diagnostics_in(unresolved_start, source.len()),
            ),
            (1, 0, 1),
            "diagnostics must stay attached to the exact Object and unresolved delegations: {:?}",
            analysis.diagnostics,
        );
    }
}
