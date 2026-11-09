// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! RAII scope guards for pattern variables and type narrowing
//!
//! This module provides guard types that ensure push/pop operations on the
//! resolver's scope stacks are always balanced, even in the presence of
//! early returns or panics.
//!
//! # Implementation
//!
//! These guards use RefCell-based interior mutability to allow both the guard
//! (which holds a shared reference) and the caller (which needs mutable operations)
//! to coexist safely. The scope stacks are wrapped in RefCell, enabling runtime-checked
//! borrowing instead of compile-time borrow checking.

use super::SymbolResolver;

/// RAII guard for narrowing scope that automatically pops on drop
///
/// This ensures push_narrowing_scope() and pop_narrowing_scope() are always balanced,
/// even when early returns or panics occur.
///
/// # Example
/// ```ignore
/// let _guard = NarrowingScopeGuard::new(&self.resolver);
/// // ... use self.resolver (via RefCell methods)
/// self.resolver.add_narrowed_type(...);  // Works via RefCell
/// // _guard automatically calls pop_narrowing_scope() when dropped
/// ```
pub struct NarrowingScopeGuard<'a> {
    resolver: &'a SymbolResolver,
}

impl<'a> NarrowingScopeGuard<'a> {
    /// Create a new narrowing scope guard and push the scope
    pub fn new(resolver: &'a SymbolResolver) -> Self {
        resolver.push_narrowing_scope();
        Self { resolver }
    }
}

impl Drop for NarrowingScopeGuard<'_> {
    fn drop(&mut self) {
        self.resolver.pop_narrowing_scope();
    }
}

/// RAII guard for pattern variable scope that automatically pops on drop
///
/// This ensures push_pattern_scope() and pop_pattern_scope() are always balanced,
/// even when early returns or panics occur.
///
/// # Example
/// ```ignore
/// let _guard = PatternScopeGuard::new(&self.resolver);
/// // ... use self.resolver (via RefCell methods)
/// self.resolver.add_pattern_variable(...);  // Works via RefCell
/// // _guard automatically calls pop_pattern_scope() when dropped
/// ```
pub struct PatternScopeGuard<'a> {
    resolver: &'a SymbolResolver,
}

impl<'a> PatternScopeGuard<'a> {
    /// Create a new pattern scope guard and push the scope
    pub fn new(resolver: &'a SymbolResolver) -> Self {
        resolver.push_pattern_scope();
        Self { resolver }
    }
}

impl Drop for PatternScopeGuard<'_> {
    fn drop(&mut self) {
        self.resolver.pop_pattern_scope();
    }
}
