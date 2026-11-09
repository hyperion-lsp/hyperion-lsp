// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use crate::symbol::Type;
use std::collections::HashMap;

/// Scope management for TypeChecker
///
/// Maintains a stack of variable scopes to track variable types in nested blocks.
/// Each scope maps variable names to their types, with inner scopes shadowing outer scopes.
impl super::TypeChecker {
    /// Enter a new scope (e.g., when entering a block statement)
    pub(super) fn enter_scope(&mut self) {
        // Pre-allocate for typical scope size (5-10 variables in a block)
        self.variable_types_stack.push(HashMap::with_capacity(8));
    }

    /// Exit the current scope (e.g., when leaving a block statement)
    pub(super) fn exit_scope(&mut self) {
        // Keep at least one scope (the top-level method scope)
        if self.variable_types_stack.len() > 1 {
            self.variable_types_stack.pop();
        }
    }

    /// Insert a variable into the current scope
    pub(super) fn insert_variable(&mut self, name: String, typ: Type) {
        if let Some(current_scope) = self.variable_types_stack.last_mut() {
            current_scope.insert(name, typ);
        }
    }

    /// Get a variable's type by searching from innermost to outermost scope
    pub(super) fn get_variable(&self, name: &str) -> Option<Type> {
        // Search from innermost scope outward
        for scope in self.variable_types_stack.iter().rev() {
            if let Some(typ) = scope.get(name) {
                return Some(typ.clone());
            }
        }
        None
    }
}
