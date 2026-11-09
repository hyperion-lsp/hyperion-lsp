// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scope graph implementation
//!
//! This module contains the Scope and ScopeGraph types that represent
//! the hierarchical scope structure of a Kotlin file.

use super::types::{ImportInfo, ImportKind, ScopeSymbol, SmartCast};
use rowan::TextRange;
use std::collections::HashMap;

/// A lexical scope containing symbols
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// Symbols defined in this scope
    pub(crate) symbols: HashMap<String, ScopeSymbol>,

    /// Text range of this scope
    pub(crate) range: TextRange,

    /// Parent scope (None for root scope)
    pub(crate) parent: Option<usize>,

    /// Child scopes
    pub(crate) children: Vec<usize>,

    /// Smart casts active in this scope (from `is` checks in conditions)
    pub(crate) smart_casts: Vec<SmartCast>,
}

impl Scope {
    /// Create a new scope
    pub fn new(range: TextRange, parent: Option<usize>) -> Self {
        Self {
            symbols: HashMap::new(),
            range,
            parent,
            children: Vec::new(),
            smart_casts: Vec::new(),
        }
    }

    /// Add a symbol to this scope
    pub fn add_symbol(&mut self, symbol: ScopeSymbol) {
        self.symbols.insert(symbol.name.clone(), symbol);
    }

    /// Get a symbol by name in this scope (doesn't check parent)
    pub fn get_symbol(&self, name: &str) -> Option<&ScopeSymbol> {
        self.symbols.get(name)
    }

    /// Add a smart cast to this scope
    pub fn add_smart_cast(&mut self, smart_cast: SmartCast) {
        self.smart_casts.push(smart_cast);
    }

    /// Get a smart cast for a variable in this scope
    pub fn get_smart_cast(&self, name: &str) -> Option<&SmartCast> {
        self.smart_casts.iter().find(|sc| sc.variable_name == name)
    }

    /// Check if a position is within this scope's range
    pub fn contains(&self, pos: usize) -> bool {
        let start = self.range.start().into();
        let end = self.range.end().into();
        pos >= start && pos < end
    }
}

/// Scope graph for a Kotlin file
///
/// Maintains a hierarchy of scopes and provides queries for symbol resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeGraph {
    /// All scopes in the file
    pub(crate) scopes: Vec<Scope>,

    /// Root scope index
    pub(crate) root: usize,

    /// Import statements in this file
    pub(crate) imports: Vec<ImportInfo>,
}

impl ScopeGraph {
    /// Create a new scope graph with a root scope
    pub fn new(file_range: TextRange) -> Self {
        let root_scope = Scope::new(file_range, None);
        Self {
            scopes: vec![root_scope],
            root: 0,
            imports: Vec::new(),
        }
    }

    /// Add an import to the scope graph
    pub fn add_import(&mut self, import: ImportInfo) {
        self.imports.push(import);
    }

    /// Get all imports in this file
    #[allow(clippy::missing_const_for_fn)]
    pub fn imports(&self) -> &[ImportInfo] {
        &self.imports
    }

    /// Resolve an import by simple name
    ///
    /// Returns the FQN of the imported symbol, if found.
    /// Handles explicit imports and aliased imports.
    pub fn resolve_import(&self, name: &str) -> Option<&str> {
        for import in &self.imports {
            match &import.kind {
                ImportKind::Explicit => {
                    if import.name == name {
                        return Some(&import.fqn);
                    }
                }
                ImportKind::Aliased { alias } => {
                    if alias == name {
                        return Some(&import.fqn);
                    }
                }
                ImportKind::Wildcard => {
                    // Wildcards don't map to specific names
                }
            }
        }
        None
    }

    /// Get all wildcard import packages
    pub fn wildcard_imports(&self) -> Vec<&str> {
        self.imports
            .iter()
            .filter_map(|import| match &import.kind {
                ImportKind::Wildcard => Some(import.fqn.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Add a child scope to a parent
    pub fn add_child_scope(&mut self, parent: usize, range: TextRange) -> usize {
        let scope_id = self.scopes.len();
        let scope = Scope::new(range, Some(parent));
        self.scopes.push(scope);
        self.scopes[parent].children.push(scope_id);
        scope_id
    }

    /// Add a symbol to a scope
    pub fn add_symbol_to_scope(&mut self, scope_id: usize, symbol: ScopeSymbol) {
        if let Some(scope) = self.scopes.get_mut(scope_id) {
            scope.add_symbol(symbol);
        }
    }

    /// Add a smart cast to a scope
    pub fn add_smart_cast_to_scope(&mut self, scope_id: usize, smart_cast: SmartCast) {
        if let Some(scope) = self.scopes.get_mut(scope_id) {
            scope.add_smart_cast(smart_cast);
        }
    }

    /// Resolve a smart cast for a variable at a position
    ///
    /// Searches the scope at the position and all parent scopes.
    /// Smart casts are propagated from inner scopes (e.g., nested if blocks).
    pub fn resolve_smart_cast(&self, name: &str, pos: usize) -> Option<&SmartCast> {
        let mut scope_id = self.scope_at_position(pos)?;

        loop {
            let scope = &self.scopes[scope_id];

            // Check if there's a smart cast in this scope
            if let Some(smart_cast) = scope.get_smart_cast(name) {
                return Some(smart_cast);
            }

            // Move to parent scope
            {
                let parent = scope.parent?;
                scope_id = parent;
            }
        }
    }

    /// Find the innermost scope containing a position
    pub fn scope_at_position(&self, pos: usize) -> Option<usize> {
        fn find_innermost(scopes: &[Scope], scope_id: usize, pos: usize) -> Option<usize> {
            let scope = &scopes[scope_id];

            if !scope.contains(pos) {
                return None;
            }

            // Check children for a more specific scope
            for &child_id in &scope.children {
                if let Some(inner) = find_innermost(scopes, child_id, pos) {
                    return Some(inner);
                }
            }

            // This is the innermost scope containing the position
            Some(scope_id)
        }

        find_innermost(&self.scopes, self.root, pos)
    }

    /// Resolve a symbol at a position
    ///
    /// Searches the scope at the position and all parent scopes.
    pub fn resolve_symbol(&self, name: &str, pos: usize) -> Option<&ScopeSymbol> {
        let mut scope_id = self.scope_at_position(pos)?;

        loop {
            let scope = &self.scopes[scope_id];

            // Check if symbol is in this scope
            if let Some(symbol) = scope.get_symbol(name) {
                return Some(symbol);
            }

            // Move to parent scope
            {
                let parent = scope.parent?;
                scope_id = parent;
            }
        }
    }

    /// Get all symbols in scope at a position (for completion)
    pub fn symbols_in_scope(&self, pos: usize) -> Vec<&ScopeSymbol> {
        let mut symbols = Vec::new();
        let Some(mut scope_id) = self.scope_at_position(pos) else {
            return symbols;
        };

        loop {
            let scope = &self.scopes[scope_id];

            // Add all symbols from this scope
            symbols.extend(scope.symbols.values());

            // Move to parent scope
            match scope.parent {
                Some(parent) => scope_id = parent,
                None => break,
            }
        }

        symbols
    }
}
