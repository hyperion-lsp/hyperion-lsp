// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbolic taint domain for interprocedural analysis.
//!
//! Tracks which parameters and sources influence a value, enabling
//! method summary generation for cross-file taint analysis.

use crate::taint::VulnTypes;
use bitflags::bitflags;

bitflags! {
    /// Tracks which origins influence a value.
    ///
    /// This is used during method summary generation to determine
    /// which parameters or external sources affect the return value.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TaintOrigins: u32 {
        /// Parameter 0 influences this value.
        const PARAM_0 = 1 << 0;
        /// Parameter 1 influences this value.
        const PARAM_1 = 1 << 1;
        /// Parameter 2 influences this value.
        const PARAM_2 = 1 << 2;
        /// Parameter 3 influences this value.
        const PARAM_3 = 1 << 3;
        /// Parameter 4 influences this value.
        const PARAM_4 = 1 << 4;
        /// Parameter 5 influences this value.
        const PARAM_5 = 1 << 5;
        /// Parameter 6 influences this value.
        const PARAM_6 = 1 << 6;
        /// Parameter 7 influences this value.
        const PARAM_7 = 1 << 7;

        /// HTTP request data (parameters, headers, body).
        const SOURCE_HTTP = 1 << 8;
        /// Database query results.
        const SOURCE_DB = 1 << 9;
        /// File system reads.
        const SOURCE_FILE = 1 << 10;
        /// Environment variables or system properties.
        const SOURCE_ENV = 1 << 11;
        /// Deserialized objects.
        const SOURCE_DESER = 1 << 12;
    }
}

impl TaintOrigins {
    /// Get the origin flag for a parameter index.
    ///
    /// Returns `None` if the index is out of range (> 7).
    #[must_use]
    pub const fn param(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::PARAM_0),
            1 => Some(Self::PARAM_1),
            2 => Some(Self::PARAM_2),
            3 => Some(Self::PARAM_3),
            4 => Some(Self::PARAM_4),
            5 => Some(Self::PARAM_5),
            6 => Some(Self::PARAM_6),
            7 => Some(Self::PARAM_7),
            _ => None,
        }
    }

    /// Get all parameter indices that influence this value.
    #[must_use]
    pub fn influenced_by_params(&self) -> Vec<usize> {
        let mut params = Vec::new();
        if self.contains(Self::PARAM_0) {
            params.push(0);
        }
        if self.contains(Self::PARAM_1) {
            params.push(1);
        }
        if self.contains(Self::PARAM_2) {
            params.push(2);
        }
        if self.contains(Self::PARAM_3) {
            params.push(3);
        }
        if self.contains(Self::PARAM_4) {
            params.push(4);
        }
        if self.contains(Self::PARAM_5) {
            params.push(5);
        }
        if self.contains(Self::PARAM_6) {
            params.push(6);
        }
        if self.contains(Self::PARAM_7) {
            params.push(7);
        }
        params
    }

    /// Check if any external source influences this value.
    #[must_use]
    pub fn has_any_source(&self) -> bool {
        self.intersects(
            Self::SOURCE_HTTP
                | Self::SOURCE_DB
                | Self::SOURCE_FILE
                | Self::SOURCE_ENV
                | Self::SOURCE_DESER,
        )
    }

    /// Check if any parameter influences this value.
    #[must_use]
    pub fn has_any_param(&self) -> bool {
        self.intersects(
            Self::PARAM_0
                | Self::PARAM_1
                | Self::PARAM_2
                | Self::PARAM_3
                | Self::PARAM_4
                | Self::PARAM_5
                | Self::PARAM_6
                | Self::PARAM_7,
        )
    }

    /// All parameter origin flags.
    const ALL_PARAMS: Self = Self::PARAM_0
        .union(Self::PARAM_1)
        .union(Self::PARAM_2)
        .union(Self::PARAM_3)
        .union(Self::PARAM_4)
        .union(Self::PARAM_5)
        .union(Self::PARAM_6)
        .union(Self::PARAM_7);

    /// Get only the parameter origins.
    #[must_use]
    pub fn params_only(&self) -> Self {
        *self & Self::ALL_PARAMS
    }

    /// Get only the source origins.
    #[must_use]
    pub fn sources_only(&self) -> Self {
        *self & !Self::ALL_PARAMS
    }
}

/// Symbolic taint state for method summary generation.
///
/// Unlike `TaintState` which tracks concrete taint flows,
/// `SymbolicTaint` tracks which parameters and sources
/// could potentially influence a value, without needing
/// to know the actual tainted values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolicTaint {
    /// Which origins (parameters/sources) influence this value.
    pub origins: TaintOrigins,
    /// Which vulnerability types this value is dangerous for.
    pub dangerous_for: VulnTypes,
    /// Whether this value is a compile-time constant.
    pub is_constant: bool,
}

impl Default for SymbolicTaint {
    fn default() -> Self {
        Self::clean()
    }
}

impl SymbolicTaint {
    /// Create a clean (untainted) symbolic state.
    #[must_use]
    pub const fn clean() -> Self {
        Self {
            origins: TaintOrigins::empty(),
            dangerous_for: VulnTypes::empty(),
            is_constant: false,
        }
    }

    /// Create a constant (definitely clean) symbolic state.
    #[must_use]
    pub const fn constant() -> Self {
        Self {
            origins: TaintOrigins::empty(),
            dangerous_for: VulnTypes::empty(),
            is_constant: true,
        }
    }

    /// Create a symbolic taint from a parameter.
    #[must_use]
    pub fn from_param(index: usize, dangerous_for: VulnTypes) -> Self {
        Self {
            origins: TaintOrigins::param(index).unwrap_or(TaintOrigins::empty()),
            dangerous_for,
            is_constant: false,
        }
    }

    /// Create a symbolic taint from an external source.
    #[must_use]
    pub const fn from_source(source: TaintOrigins, dangerous_for: VulnTypes) -> Self {
        Self {
            origins: source,
            dangerous_for,
            is_constant: false,
        }
    }

    /// Merge two symbolic taint states.
    ///
    /// The result is influenced by all origins from both states,
    /// and dangerous for all vulnerability types from both states.
    /// If either is not constant, the result is not constant.
    #[must_use]
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            origins: self.origins | other.origins,
            dangerous_for: self.dangerous_for | other.dangerous_for,
            is_constant: self.is_constant && other.is_constant,
        }
    }

    /// Check if this value is tainted (influenced by any origin).
    #[must_use]
    pub const fn is_tainted(&self) -> bool {
        !self.is_constant && !self.origins.is_empty()
    }

    /// Check if this value is influenced by any parameter.
    #[must_use]
    pub fn is_param_influenced(&self) -> bool {
        self.origins.has_any_param()
    }

    /// Check if this value is influenced by any external source.
    #[must_use]
    pub fn is_source_influenced(&self) -> bool {
        self.origins.has_any_source()
    }

    /// Get the parameter indices that influence this value.
    #[must_use]
    pub fn influenced_params(&self) -> Vec<usize> {
        self.origins.influenced_by_params()
    }

    /// Apply sanitization for specific vulnerability types.
    #[must_use]
    pub fn sanitize(&self, sanitizes: VulnTypes) -> Self {
        let remaining = self.dangerous_for & !sanitizes;
        if remaining.is_empty() && !self.origins.has_any_source() {
            Self::clean()
        } else {
            Self {
                origins: self.origins,
                dangerous_for: remaining,
                is_constant: self.is_constant,
            }
        }
    }

    /// Mark this value as constant, making it clean.
    #[must_use]
    pub const fn as_constant(&self) -> Self {
        Self {
            origins: TaintOrigins::empty(),
            dangerous_for: VulnTypes::empty(),
            is_constant: true,
        }
    }

    /// Propagate taint through an operation.
    ///
    /// The result inherits the origins and dangerous_for flags,
    /// but is no longer constant.
    #[must_use]
    pub const fn propagate(&self) -> Self {
        if self.is_constant {
            Self::constant()
        } else {
            Self {
                origins: self.origins,
                dangerous_for: self.dangerous_for,
                is_constant: false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_taint_origins_param() {
        assert_eq!(TaintOrigins::param(0), Some(TaintOrigins::PARAM_0));
        assert_eq!(TaintOrigins::param(7), Some(TaintOrigins::PARAM_7));
        assert_eq!(TaintOrigins::param(8), None);
    }

    #[test]
    fn test_taint_origins_influenced_by_params() {
        let origins = TaintOrigins::PARAM_0 | TaintOrigins::PARAM_2 | TaintOrigins::SOURCE_HTTP;
        let params = origins.influenced_by_params();
        assert_eq!(params, vec![0, 2]);
    }

    #[test]
    fn test_taint_origins_has_any_source() {
        let no_source = TaintOrigins::PARAM_0 | TaintOrigins::PARAM_1;
        assert!(!no_source.has_any_source());

        let with_source = TaintOrigins::PARAM_0 | TaintOrigins::SOURCE_HTTP;
        assert!(with_source.has_any_source());
    }

    #[test]
    fn test_symbolic_taint_clean() {
        let taint = SymbolicTaint::clean();
        assert!(!taint.is_tainted());
        assert!(!taint.is_constant);
        assert!(taint.origins.is_empty());
    }

    #[test]
    fn test_symbolic_taint_constant() {
        let taint = SymbolicTaint::constant();
        assert!(!taint.is_tainted());
        assert!(taint.is_constant);
    }

    #[test]
    fn test_symbolic_taint_from_param() {
        let taint = SymbolicTaint::from_param(0, VulnTypes::SQL_INJECTION);
        assert!(taint.is_tainted());
        assert!(taint.origins.contains(TaintOrigins::PARAM_0));
        assert!(taint.dangerous_for.contains(VulnTypes::SQL_INJECTION));
    }

    #[test]
    fn test_symbolic_taint_merge() {
        let taint1 = SymbolicTaint::from_param(0, VulnTypes::SQL_INJECTION);
        let taint2 = SymbolicTaint::from_param(1, VulnTypes::XSS);

        let merged = taint1.merge(&taint2);
        assert!(merged.origins.contains(TaintOrigins::PARAM_0));
        assert!(merged.origins.contains(TaintOrigins::PARAM_1));
        assert!(merged.dangerous_for.contains(VulnTypes::SQL_INJECTION));
        assert!(merged.dangerous_for.contains(VulnTypes::XSS));
    }

    #[test]
    fn test_symbolic_taint_merge_with_constant() {
        let taint = SymbolicTaint::from_param(0, VulnTypes::SQL_INJECTION);
        let constant = SymbolicTaint::constant();

        // Tainted + constant = tainted (not constant)
        let merged = taint.merge(&constant);
        assert!(merged.is_tainted());
        assert!(!merged.is_constant);

        // Constant + constant = constant
        let both_const = constant.merge(&SymbolicTaint::constant());
        assert!(both_const.is_constant);
    }

    #[test]
    fn test_symbolic_taint_sanitize() {
        let taint = SymbolicTaint::from_param(0, VulnTypes::SQL_INJECTION | VulnTypes::XSS);

        let sanitized = taint.sanitize(VulnTypes::SQL_INJECTION);
        assert!(!sanitized.dangerous_for.contains(VulnTypes::SQL_INJECTION));
        assert!(sanitized.dangerous_for.contains(VulnTypes::XSS));
    }

    #[test]
    fn test_symbolic_taint_full_sanitize() {
        let taint = SymbolicTaint::from_param(0, VulnTypes::SQL_INJECTION);

        // Full sanitization without external sources -> clean
        let sanitized = taint.sanitize(VulnTypes::SQL_INJECTION);
        assert!(!sanitized.is_tainted());
    }
}
