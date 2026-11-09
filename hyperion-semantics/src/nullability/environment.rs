// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Nullability environment - tracks variable nullability state at program points

use super::Nullability;
use std::collections::HashMap;

/// Unique identifier for a variable in nullability analysis
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariableId(pub u32);

impl VariableId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Nullability state of all variables at a program point
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NullabilityEnv {
    /// Map from variable to its nullability at this point
    bindings: HashMap<VariableId, Nullability>,
}

impl NullabilityEnv {
    /// Create empty environment
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that a variable has a specific nullability
    pub fn set(&mut self, var: VariableId, nullability: Nullability) {
        self.bindings.insert(var, nullability);
    }

    /// Get current nullability of a variable (Unknown if not tracked)
    pub fn get(&self, var: VariableId) -> Nullability {
        self.bindings
            .get(&var)
            .copied()
            .unwrap_or(Nullability::Unknown)
    }

    /// Check if variable is being tracked
    pub fn contains(&self, var: VariableId) -> bool {
        self.bindings.contains_key(&var)
    }

    /// Retain only bindings accepted by `keep`.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(VariableId) -> bool) {
        self.bindings.retain(|variable, _| keep(*variable));
    }

    /// Get all tracked variables
    pub fn variables(&self) -> impl Iterator<Item = VariableId> + '_ {
        self.bindings.keys().copied()
    }

    /// Join two environments at control flow merge points
    ///
    /// For each variable, joins their nullability states using the lattice join.
    /// Variables only in one environment get Unknown (we don't know if they were set).
    pub fn join(&self, other: &Self) -> Self {
        let mut result = Self::new();

        // Collect all variables from both environments
        let all_vars: std::collections::HashSet<_> = self
            .bindings
            .keys()
            .chain(other.bindings.keys())
            .copied()
            .collect();

        for var in all_vars {
            let left = self.get(var);
            let right = other.get(var);
            result.set(var, left.join(right));
        }

        result
    }

    /// Create a copy with narrowed nullability for a variable
    pub fn with_narrowed(&self, var: VariableId, nullability: Nullability) -> Self {
        let mut result = self.clone();
        result.set(var, nullability);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_env() {
        let env = NullabilityEnv::new();
        assert_eq!(env.get(VariableId(0)), Nullability::Unknown);
    }

    #[test]
    fn test_set_and_get() {
        let mut env = NullabilityEnv::new();
        env.set(VariableId(0), Nullability::NonNull);
        assert_eq!(env.get(VariableId(0)), Nullability::NonNull);
    }

    #[test]
    fn test_join_same_vars() {
        let mut env1 = NullabilityEnv::new();
        env1.set(VariableId(0), Nullability::NonNull);

        let mut env2 = NullabilityEnv::new();
        env2.set(VariableId(0), Nullability::Null);

        let joined = env1.join(&env2);
        assert_eq!(joined.get(VariableId(0)), Nullability::Nullable);
    }

    #[test]
    fn test_join_different_vars() {
        let mut env1 = NullabilityEnv::new();
        env1.set(VariableId(0), Nullability::NonNull);

        let mut env2 = NullabilityEnv::new();
        env2.set(VariableId(1), Nullability::Null);

        let joined = env1.join(&env2);
        // Both should be joined with Unknown
        assert_eq!(joined.get(VariableId(0)), Nullability::Unknown);
        assert_eq!(joined.get(VariableId(1)), Nullability::Unknown);
    }
}
