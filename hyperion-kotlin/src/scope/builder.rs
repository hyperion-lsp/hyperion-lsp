// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Scope graph builder
//!
//! This module contains the ScopeGraphBuilder which constructs
//! scope graphs from Kotlin AST.

use super::graph::ScopeGraph;
use super::types::{ScopeSymbol, SmartCast};
use rowan::TextRange;

/// Builder for constructing scope graphs from AST
pub struct ScopeGraphBuilder {
    pub(crate) graph: ScopeGraph,
    pub(crate) current_scope: usize,
}

impl ScopeGraphBuilder {
    /// Create a new scope graph builder
    pub fn new(file_range: TextRange) -> Self {
        let graph = ScopeGraph::new(file_range);
        let root = graph.root;
        Self {
            graph,
            current_scope: root,
        }
    }

    /// Enter a new scope (e.g., function body, block)
    pub fn enter_scope(&mut self, range: TextRange) {
        let new_scope = self.graph.add_child_scope(self.current_scope, range);
        self.current_scope = new_scope;
    }

    /// Exit the current scope
    pub fn exit_scope(&mut self) {
        if let Some(parent) = self.graph.scopes[self.current_scope].parent {
            self.current_scope = parent;
        }
    }

    /// Add a symbol to the current scope
    pub fn add_symbol(&mut self, symbol: ScopeSymbol) {
        self.graph.add_symbol_to_scope(self.current_scope, symbol);
    }

    /// Add a smart cast to the current scope
    pub fn add_smart_cast(&mut self, smart_cast: SmartCast) {
        self.graph
            .add_smart_cast_to_scope(self.current_scope, smart_cast);
    }

    /// Resolve a symbol by name at the given position
    ///
    /// This allows looking up variables during scope building, which is useful
    /// for inferring lambda 'it' types when the receiver variable is already defined.
    pub fn resolve_symbol(&self, name: &str, pos: usize) -> Option<&ScopeSymbol> {
        self.graph.resolve_symbol(name, pos)
    }

    /// Build the scope graph
    pub fn build(self) -> ScopeGraph {
        self.graph
    }
}
