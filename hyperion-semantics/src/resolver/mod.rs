// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbol Resolution and Scope Management
//!
//! This module provides the **symbol resolution engine** that resolves names (identifiers)
//! to their declarations within appropriate scopes. It works with the symbol table defined
//! in `symbol.rs` to provide incremental, query-based symbol resolution for LSP features
//! like goto-definition, completion, and hover.
//!
//! # Symbol Resolution Order
//!
//! When resolving an identifier like `foo`, the resolver searches in this order (JLS §6.3):
//!
//! 1. **Pattern Variables** (Java 16+): Instanceof pattern variables (innermost scope first)
//! 2. **Narrowed Types**: Smart cast types from instanceof checks (e.g., `if (obj instanceof String)`)
//! 3. **Local Scopes**: Walk scope stack from innermost to outermost (locals, parameters, fields)
//! 4. **Explicit Imports**: Check imported symbols (e.g., `import com.example.User;`)
//! 5. **Wildcard Imports**: Check wildcard imports (e.g., `import com.example.*;`)
//! 6. **Same Package**: Look for classes in the same package
//! 7. **java.lang**: Implicit import (e.g., `String`, `Object`, `System`)
//!
//! This order matches JLS §6.3 (Scope of a Declaration) and ensures correct shadowing behavior.
//!
//! # Scope Management
//!
//! The resolver maintains a **scope stack** that mirrors the lexical structure of Java code:
//!
//! ```text
//! Scope Stack (innermost scope is last):
//! [File] → [Class] → [Method] → [Block] → [If-Statement]
//!   ↑        ↑         ↑          ↑          ↑
//!   │        │         │          │          └─ Local variables in if-block
//!   │        │         │          └─────────── Local variables in method
//!   │        │         └───────────────────── Method parameters
//!   │        └─────────────────────────────── Class fields
//!   └──────────────────────────────────────── Top-level classes
//! ```
//!
//! **Scope Kinds:**
//! - `File`: Top-level scope (contains package and imports)
//! - `Class`: Class or interface scope (contains fields, methods, nested classes)
//! - `Method`: Method scope (contains parameters and local variables)
//! - `Block`: Block scope (contains local variables within `{ ... }`)
//! - `Loop`: Loop scope (contains loop variables like `for (int i = 0; ...)`)
//!
//! # Scope Narrowing (Smart Cast)
//!
//! The resolver tracks **narrowed types** from instanceof checks to enable smart completion:
//!
//! ```java
//! Object obj = getObject();
//! if (obj instanceof String) {
//!     obj.| // <-- obj is narrowed to String here
//! }
//! ```
//!
//! **Implementation:**
//! - `push_narrowing_scope()` / `pop_narrowing_scope()`: Manage narrowing scopes (use RAII guard!)
//! - `add_narrowed_type(name, type)`: Register a narrowed type in current scope
//! - `get_narrowed_type(name)`: Look up narrowed type (searches innermost to outermost)
//!
//! **Safe Scope Management:**
//! Use closure-based methods or RAII guards to ensure push/pop balance:
//!
//! ```ignore
//! // Closure-based API (safe, automatic cleanup)
//! resolver.with_narrowing_scope(|| {
//!     resolver.add_narrowed_type("obj".to_string(), Type::Class(ClassType::string()));
//!     // ... do work with narrowed type
//! }); // Scope automatically cleaned up here, even on panic
//!
//! // Or use RAII guards (also safe, no unsafe code)
//! let _guard = NarrowingScopeGuard::new(&resolver);
//! resolver.add_narrowed_type("obj".to_string(), Type::Class(ClassType::string()));
//! // Guard automatically pops scope when dropped
//! ```
//!
//! # Nested Class Resolution
//!
//! Nested classes (inner classes, static nested classes) are resolved using qualified names:
//!
//! ```java
//! class Outer {
//!     class Inner {
//!         void method() {
//!             Outer.Inner inner = new Inner(); // Both resolve to Outer.Inner
//!         }
//!     }
//! }
//! ```
//!
//! **Resolution strategy:**
//! 1. Try simple name: `Inner` → check current class and enclosing classes
//! 2. Try qualified name: `Outer.Inner` → check WorkspaceIndex for FQN
//! 3. Try fully qualified name: `com.example.Outer.Inner` → direct WorkspaceIndex lookup
//!
//! # Method Overload Resolution
//!
//! When resolving method calls, the resolver delegates to the canonical
//! [`crate::call_resolution`] contract. That contract collects inherited candidates,
//! applies Java invocation conversions in phases, chooses the most-specific applicable
//! declaration, and carries generic substitutions into the resolved return type.
//!
//! **Example:**
//! ```java
//! void foo(String s) { ... }
//! void foo(Object o) { ... }
//!
//! foo("hello"); // Resolves to foo(String) - more specific match
//! ```
//!
//! In incomplete editing states, individual consumers may expose an explicit degraded
//! fallback, but compatibility and overload ranking remain owned by the shared contract.
//!
//! # Architecture
//!
//! ```text
//! SymbolResolver
//! ├── Core Components
//! │   ├── symbol_table: Arc<SymbolTable> - Shared symbol table (Salsa-cached)
//! │   ├── scope_stack: Vec<ScopeId> - Current scope chain (innermost last)
//! │   └── resolution_cache: (ScopeId, Name) → SymbolId - Per-request cache
//! ├── Narrowing & Patterns
//! │   ├── pattern_variables_stack: Vec<HashMap<Name, Type>> - Instanceof patterns
//! │   └── narrowed_types_stack: Vec<HashMap<Name, Type>> - Smart cast types
//! └── Cross-File Lookup
//!     └── workspace_index: Arc<WorkspaceIndex> - Cross-file symbol queries
//! ```
//!
//! # Example
//!
//! ```rust
//! use hyperion_semantics::resolver::SymbolResolver;
//! use hyperion_semantics::symbol::{SymbolTable, ScopeKind};
//!
//! # fn example() {
//! // Create resolver with a symbol table
//! let mut resolver = SymbolResolver::new();
//!
//! // Enter a class scope
//! let class_scope = resolver.symbol_table_mut().create_scope(ScopeKind::Class, None);
//! // ... define and resolve symbols
//! # }
//! ```

// Sub-modules
pub mod builder;
mod scope;
mod symbol;
mod type_resolution;
pub use type_resolution::{resolve_effective_field, resolve_effective_field_for_owner};

// Builder sub-modules (used by builder.rs)
mod method_reference;
mod method_resolution;
pub(crate) mod type_inference;
pub mod type_parameter_mapping;

// Re-exports
pub use builder::ResolverBuilder;
pub use scope::{NarrowingScopeGuard, PatternScopeGuard};

use crate::db::FileId;
use crate::symbol::{ScopeId, SymbolId, SymbolTable, Type};
use hyperion_workspace::WorkspaceIndex;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// A fully resolved symbol with its type information
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSymbol {
    /// The symbol ID in the symbol table
    pub symbol_id: SymbolId,
    /// The symbol's name
    pub name: String,
    /// The symbol's type
    pub ty: Type,
    /// The file where the symbol is defined
    pub file_id: FileId,
    /// The scope where the symbol is defined
    pub scope_id: ScopeId,
}

/// Symbol resolution engine that walks scope chains
pub struct SymbolResolver {
    /// The symbol table containing all symbols and scopes.
    /// Stored as Arc to allow sharing cached tables without cloning.
    pub(super) symbol_table: Arc<SymbolTable>,
    /// Current scope stack (innermost scope is last)
    pub(super) scope_stack: Vec<ScopeId>,
    /// Pattern variables from instanceof expressions (Java 16+)
    /// Stack of scopes, each with their own pattern variables.
    /// Maps variable name to its type for quick type lookup during method resolution.
    /// Wrapped in RefCell to allow interior mutability for scope push/pop operations.
    pub(crate) pattern_variables_stack: RefCell<Vec<HashMap<String, Type>>>,
    /// Narrowed types from instanceof checks (smart cast)
    /// Maps variable name to its narrowed type within a scope.
    /// Example: after `if (obj instanceof String)`, obj is narrowed to String in the then-block.
    /// Stack of scopes, each with their own narrowed types.
    /// Wrapped in RefCell to allow interior mutability for scope push/pop operations.
    pub(crate) narrowed_types_stack: RefCell<Vec<HashMap<String, Type>>>,
    /// Cache for symbol resolution (scope + name -> symbol ID)
    /// This cache is per-request, not stored in Salsa's cache, to avoid stale cache issues.
    pub(super) resolution_cache: HashMap<(ScopeId, String), Option<SymbolId>>,
    /// Workspace index for cross-file symbol lookup (optional)
    /// Used by get_method_return_type and resolve_field_type
    pub(super) workspace_index: Option<Arc<WorkspaceIndex>>,
}

impl SymbolResolver {
    /// Create a new symbol resolver with an empty symbol table
    pub fn new() -> Self {
        let symbol_table = Arc::new(SymbolTable::new());
        let root_scope = symbol_table.root_scope();

        Self {
            symbol_table,
            scope_stack: vec![root_scope],
            pattern_variables_stack: RefCell::new(vec![HashMap::new()]),
            narrowed_types_stack: RefCell::new(vec![HashMap::new()]),
            resolution_cache: HashMap::new(),
            workspace_index: None,
        }
    }

    /// Create a symbol resolver with an existing symbol table (takes ownership)
    pub fn with_symbol_table(symbol_table: SymbolTable) -> Self {
        let symbol_table = Arc::new(symbol_table);
        let root_scope = symbol_table.root_scope();

        Self {
            symbol_table,
            scope_stack: vec![root_scope],
            pattern_variables_stack: RefCell::new(vec![HashMap::new()]),
            narrowed_types_stack: RefCell::new(vec![HashMap::new()]),
            resolution_cache: HashMap::new(),
            workspace_index: None,
        }
    }

    /// Create a symbol resolver with a cached/shared symbol table (avoids clone)
    ///
    /// Use this when working with Salsa-cached symbol tables to avoid the
    /// "clone tax" of copying the entire table.
    pub fn with_cached_table(symbol_table: Arc<SymbolTable>) -> Self {
        let root_scope = symbol_table.root_scope();

        Self {
            symbol_table,
            scope_stack: vec![root_scope],
            pattern_variables_stack: RefCell::new(vec![HashMap::new()]),
            narrowed_types_stack: RefCell::new(vec![HashMap::new()]),
            resolution_cache: HashMap::new(),
            workspace_index: None,
        }
    }

    /// Set the workspace index for cross-file symbol lookup
    ///
    /// This enables `get_method_return_type` and `resolve_field_type` to resolve
    /// method and field types across files using the workspace index.
    pub fn set_workspace_index(&mut self, workspace_index: Arc<WorkspaceIndex>) {
        self.workspace_index = Some(workspace_index);
    }

    /// Get a reference to the underlying symbol table
    pub fn symbol_table(&self) -> &SymbolTable {
        &self.symbol_table
    }

    /// Get a mutable reference to the underlying symbol table.
    ///
    /// Note: This uses Arc::make_mut, which will clone the table if there
    /// are other references to it. For read-only operations, prefer using
    /// `symbol_table()` instead.
    ///
    /// **IMPORTANT**: This automatically clears the resolution cache to prevent
    /// stale symbol lookups after symbol table mutations.
    pub fn symbol_table_mut(&mut self) -> &mut SymbolTable {
        // Invalidate cache when symbol table is mutated
        self.resolution_cache.clear();
        Arc::make_mut(&mut self.symbol_table)
    }

    /// Get the type of a pattern variable (Java 16+ instanceof pattern matching)
    ///
    /// Returns the type if the variable is a pattern variable, None otherwise.
    /// Pattern variables are created by `instanceof` expressions like:
    /// `if (obj instanceof String s) { ... }`
    ///
    /// Searches from innermost scope to outermost.
    pub fn get_pattern_variable_type(&self, name: &str) -> Option<Type> {
        let stack = self.pattern_variables_stack.borrow();
        // Search from innermost scope (last) to outermost (first)
        for scope_patterns in stack.iter().rev() {
            if let Some(ty) = scope_patterns.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }

    /// Get the narrowed type for a variable (smart cast from instanceof)
    ///
    /// Returns the narrowed type if the variable has been narrowed by an instanceof check.
    /// This enables smart completion after instanceof checks like:
    /// `if (obj instanceof String) { obj.| }` - obj is narrowed to String
    ///
    /// Searches from innermost scope to outermost.
    pub fn get_narrowed_type(&self, name: &str) -> Option<Type> {
        let stack = self.narrowed_types_stack.borrow();
        // Search from innermost scope (last) to outermost (first)
        for scope_narrowings in stack.iter().rev() {
            if let Some(ty) = scope_narrowings.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }

    /// Get all narrowed types in the current context (for completion)
    ///
    /// Returns a map of variable names to their narrowed types.
    /// Merges all scopes, with inner scopes taking precedence.
    pub fn get_all_narrowed_types(&self) -> HashMap<String, Type> {
        let stack = self.narrowed_types_stack.borrow();
        let mut result = HashMap::new();
        // Process from outermost to innermost so inner scopes override
        for scope_narrowings in stack.iter() {
            for (name, ty) in scope_narrowings {
                result.insert(name.clone(), ty.clone());
            }
        }
        result
    }

    // ==================== Pattern Variable & Narrowing Scope Management ====================
    //
    // These methods manage scoped type information for instanceof checks and pattern variables.
    // Each push MUST have a matching pop to prevent scope stack leaks.
    //
    // Stack leak symptoms:
    // - Memory growth in long-running LSP sessions
    // - Pattern variables bleeding into unrelated scopes
    // - Completion suggesting out-of-scope pattern variables
    // - Smart cast type narrowing giving false positives
    //
    // The debug_assert! calls detect unbalanced push/pop in development.
    // In production (release builds), the guards prevent popping the root scope.
    //
    // Usage pattern:
    //   self.push_narrowing_scope();
    //   // ... process narrowed types
    //   self.pop_narrowing_scope();  // MUST always be called
    //
    // CRITICAL: Always ensure pop is called, even on early returns or errors.
    // Consider using a block scope or explicit error handling to ensure cleanup.
    // ========================================================================================

    /// Push a new narrowing scope (called when entering a then-block after instanceof)
    ///
    /// IMPORTANT: Every call to this method MUST have a matching `pop_narrowing_scope()`
    /// call, even in the presence of early returns or errors.
    ///
    /// DEPRECATED: Prefer using `with_narrowing_scope` instead for automatic cleanup.
    pub fn push_narrowing_scope(&self) {
        self.narrowed_types_stack.borrow_mut().push(HashMap::new());
    }

    /// Pop the current narrowing scope (called when exiting a then-block)
    ///
    /// IMPORTANT: This should only be called after a matching `push_narrowing_scope()`.
    /// The debug assertion will catch unbalanced calls in development builds.
    ///
    /// DEPRECATED: Prefer using `with_narrowing_scope` instead for automatic cleanup.
    pub fn pop_narrowing_scope(&self) {
        let mut stack = self.narrowed_types_stack.borrow_mut();
        debug_assert!(
            stack.len() > 1,
            "Unbalanced narrowing scope pop - attempting to pop root scope. Stack has {} elements. \
             This indicates a logic bug where pop was called without a matching push.",
            stack.len()
        );
        if stack.len() > 1 {
            stack.pop();
        }
    }

    /// Execute a closure with a new narrowing scope (safe RAII alternative)
    ///
    /// This method ensures that the narrowing scope is properly cleaned up,
    /// even in the presence of early returns or panics. This is the preferred
    /// way to work with narrowing scopes.
    ///
    /// # Example
    /// ```ignore
    /// resolver.with_narrowing_scope(|| {
    ///     resolver.add_narrowed_type("obj".to_string(), Type::Class(ClassType::string()));
    ///     // ... do work with narrowed type
    /// }); // Scope automatically cleaned up here
    /// ```
    pub fn with_narrowing_scope<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.push_narrowing_scope();
        let result = f();
        self.pop_narrowing_scope();
        result
    }

    /// Add a narrowed type for a variable in the current scope
    pub fn add_narrowed_type(&self, name: String, ty: Type) {
        let mut stack = self.narrowed_types_stack.borrow_mut();
        if let Some(current) = stack.last_mut() {
            current.insert(name, ty);
        }
    }

    /// Push a new pattern variable scope (called when entering a then-block with pattern vars)
    ///
    /// IMPORTANT: Every call to this method MUST have a matching `pop_pattern_scope()`
    /// call, even in the presence of early returns or errors.
    ///
    /// DEPRECATED: Prefer using `with_pattern_scope` instead for automatic cleanup.
    pub fn push_pattern_scope(&self) {
        self.pattern_variables_stack
            .borrow_mut()
            .push(HashMap::new());
    }

    /// Pop the current pattern variable scope (called when exiting a then-block)
    ///
    /// IMPORTANT: This should only be called after a matching `push_pattern_scope()`.
    /// The debug assertion will catch unbalanced calls in development builds.
    ///
    /// DEPRECATED: Prefer using `with_pattern_scope` instead for automatic cleanup.
    pub fn pop_pattern_scope(&self) {
        let mut stack = self.pattern_variables_stack.borrow_mut();
        debug_assert!(
            stack.len() > 1,
            "Unbalanced pattern scope pop - attempting to pop root scope. Stack has {} elements. \
             This indicates a logic bug where pop was called without a matching push.",
            stack.len()
        );
        if stack.len() > 1 {
            stack.pop();
        }
    }

    /// Execute a closure with a new pattern variable scope (safe RAII alternative)
    ///
    /// This method ensures that the pattern scope is properly cleaned up,
    /// even in the presence of early returns or panics. This is the preferred
    /// way to work with pattern variable scopes.
    ///
    /// # Example
    /// ```ignore
    /// resolver.with_pattern_scope(|| {
    ///     resolver.add_pattern_variable("s".to_string(), Type::Class(ClassType::string()));
    ///     // ... do work with pattern variable
    /// }); // Scope automatically cleaned up here
    /// ```
    pub fn with_pattern_scope<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.push_pattern_scope();
        let result = f();
        self.pop_pattern_scope();
        result
    }

    /// Add a pattern variable to the current scope
    pub fn add_pattern_variable(&self, name: String, ty: Type) {
        let mut stack = self.pattern_variables_stack.borrow_mut();
        if let Some(current) = stack.last_mut() {
            current.insert(name, ty);
        }
    }
}

impl Default for SymbolResolver {
    fn default() -> Self {
        Self::new()
    }
}
