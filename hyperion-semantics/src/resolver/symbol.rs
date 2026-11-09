// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbol lookup and resolution methods
//!
//! This module provides methods for resolving symbols by name, managing
//! scopes, and querying the symbol table.

use super::{ResolvedSymbol, SymbolResolver};
use crate::symbol::{Scope, ScopeId, ScopeKind, Symbol, SymbolId};
use hyperion_parser::syntax::SyntaxNode;

impl SymbolResolver {
    /// Enter a new scope, making it the current scope
    pub fn enter_scope(&mut self, kind: ScopeKind) -> ScopeId {
        let parent = self.current_scope_id();
        let scope_id = self.symbol_table_mut().create_scope(kind, Some(parent));
        self.scope_stack.push(scope_id);
        scope_id
    }

    /// Exit the current scope and return to the parent scope
    /// Returns false if already at root scope
    ///
    /// Clear resolution cache on scope exit to prevent stale lookups.
    /// When we exit a child scope and return to parent, any cached lookups from
    /// the child scope could be stale if the parent was modified.
    pub fn exit_scope(&mut self) -> bool {
        if self.scope_stack.len() > 1 {
            self.scope_stack.pop();
            self.resolution_cache.clear(); // Clear cache on scope exit
            true
        } else {
            false
        }
    }

    /// Get the current (innermost) scope ID
    pub fn current_scope_id(&self) -> ScopeId {
        *self
            .scope_stack
            .last()
            .expect("Scope stack should never be empty")
    }

    /// Get the current scope
    pub fn current_scope(&self) -> Option<&Scope> {
        let scope_id = self.current_scope_id();
        self.symbol_table.get_scope(scope_id)
    }

    /// Define a symbol in the current scope
    pub fn define_symbol(&mut self, symbol: Symbol) -> SymbolId {
        let scope_id = self.current_scope_id();
        self.symbol_table_mut().add_symbol(symbol, scope_id)
    }

    /// Cached symbol lookup - checks cache first, then delegates to SymbolTable
    pub(super) fn cached_lookup(&mut self, name: &str, scope: ScopeId) -> Option<SymbolId> {
        let cache_key = (scope, name.to_string());
        if let Some(cached) = self.resolution_cache.get(&cache_key) {
            return *cached;
        }

        let result = self.symbol_table.lookup_symbol(name, scope);
        self.resolution_cache.insert(cache_key, result);
        result
    }

    /// Resolve a symbol by name, searching from the current scope upward
    pub fn resolve(&mut self, name: &str) -> Option<ResolvedSymbol> {
        let current_scope = self.current_scope_id();
        let symbol_id = self.cached_lookup(name, current_scope)?;
        let symbol = self.symbol_table.get_symbol(symbol_id)?;

        Some(ResolvedSymbol {
            symbol_id,
            name: symbol.name.clone(),
            ty: symbol.type_info.clone(),
            file_id: symbol.location.file_id,
            scope_id: current_scope,
        })
    }

    /// Resolve a symbol in a specific scope
    pub fn resolve_in_scope(&mut self, name: &str, scope_id: ScopeId) -> Option<ResolvedSymbol> {
        let symbol_id = self.cached_lookup(name, scope_id)?;
        let symbol = self.symbol_table.get_symbol(symbol_id)?;

        Some(ResolvedSymbol {
            symbol_id,
            name: symbol.name.clone(),
            ty: symbol.type_info.clone(),
            file_id: symbol.location.file_id,
            scope_id,
        })
    }

    /// Resolve a symbol for goto-definition, preferring fields over parameters.
    ///
    /// This method is specifically for navigation use cases where users expect
    /// to navigate to the field declaration rather than a shadowing parameter.
    pub fn resolve_in_scope_for_navigation(
        &self,
        name: &str,
        scope_id: ScopeId,
    ) -> Option<ResolvedSymbol> {
        let symbol_id = self
            .symbol_table
            .lookup_symbol_for_navigation(name, scope_id)?;
        let symbol = self.symbol_table.get_symbol(symbol_id)?;

        Some(ResolvedSymbol {
            symbol_id,
            name: symbol.name.clone(),
            ty: symbol.type_info.clone(),
            file_id: symbol.location.file_id,
            scope_id,
        })
    }

    /// Resolve at a concrete source offset, respecting point-of-declaration
    /// visibility for local variables and local types.
    pub fn resolve_in_scope_at_offset(
        &self,
        name: &str,
        scope_id: ScopeId,
        offset: u32,
    ) -> Option<ResolvedSymbol> {
        let symbol_id = self
            .symbol_table
            .lookup_symbol_at_offset(name, scope_id, offset)?;
        let symbol = self.symbol_table.get_symbol(symbol_id)?;

        Some(ResolvedSymbol {
            symbol_id,
            name: symbol.name.clone(),
            ty: symbol.type_info.clone(),
            file_id: symbol.location.file_id,
            scope_id,
        })
    }

    /// Get all symbols visible in the current scope
    pub fn visible_symbols(&self) -> Vec<(String, SymbolId)> {
        let current_scope = self.current_scope_id();
        self.symbol_table.symbols_in_scope(current_scope)
    }

    /// Get the enclosing class scope, if any
    pub fn enclosing_class_scope(&self) -> Option<ScopeId> {
        let current_scope = self.current_scope_id();
        self.symbol_table.find_enclosing_class(current_scope)
    }

    /// Get the enclosing method scope, if any
    pub fn enclosing_method_scope(&self) -> Option<ScopeId> {
        let current_scope = self.current_scope_id();
        self.symbol_table.find_enclosing_method(current_scope)
    }

    /// Resolve a symbol by searching all scopes (fallback when normal resolution fails)
    ///
    /// This is useful for goto-definition where we need to find symbols anywhere in the file,
    /// not just in the current scope chain.
    pub fn resolve_anywhere(&mut self, name: &str) -> Option<ResolvedSymbol> {
        let symbol_id = self.symbol_table.find_symbol_anywhere(name)?;
        let symbol = self.symbol_table.get_symbol(symbol_id)?;

        Some(ResolvedSymbol {
            symbol_id,
            name: symbol.name.clone(),
            ty: symbol.type_info.clone(),
            file_id: symbol.location.file_id,
            scope_id: self.current_scope_id(), // Use current scope for context
        })
    }

    /// Clear the resolution cache (useful after bulk updates)
    pub fn clear_cache(&mut self) {
        self.resolution_cache.clear();
    }

    /// Find the deepest scope that contains the given byte offset
    pub fn find_scope_at_offset(&self, offset: u32) -> Option<ScopeId> {
        self.symbol_table.find_scope_at_offset(offset)
    }

    // === Type Inference Support Methods ===

    /// Resolve an identifier to its symbol
    ///
    /// This looks up a variable, field, or parameter by name and returns
    /// its symbol information. Uses direct symbol table lookup without caching.
    pub fn resolve_identifier(&self, name: &str, _node: &SyntaxNode) -> Option<Symbol> {
        // Walk the scope stack to find the symbol
        let current_scope = self.current_scope_id();
        if let Some(symbol_id) = self.symbol_table.lookup_symbol(name, current_scope) {
            if let Some(symbol) = self.symbol_table.get_symbol(symbol_id) {
                return Some(symbol.clone());
            }
        }
        None
    }
}
