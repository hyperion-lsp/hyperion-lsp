// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Symbol Usage Statistics
//!
//! Tracks how often symbols are used throughout the workspace to enable
//! intelligent ranking in code completion.
//!
//! # Features
//! - Track symbol usage frequency by FQN
//! - Compute normalized usage scores (0.0 to 1.0)
//! - Efficient in-memory storage
//!
//! # Example
//! ```ignore
//! let mut stats = UsageStatistics::new();
//! stats.record_usage("java.util.ArrayList");
//! stats.record_usage("java.util.ArrayList"); // Used twice
//! stats.record_usage("java.util.HashMap");   // Used once
//!
//! assert!(stats.get_usage_score("java.util.ArrayList") > stats.get_usage_score("java.util.HashMap"));
//! ```

use std::collections::HashMap;
use tracing::debug;

/// Tracks symbol usage statistics for intelligent completion ranking
#[derive(Debug, Clone, Default)]
pub struct UsageStatistics {
    /// Maps symbol FQN to usage count
    symbol_usage: HashMap<String, u32>,

    /// Cached max usage count (invalidated on record_usage)
    cached_max: Option<u32>,
}

impl UsageStatistics {
    /// Create a new empty usage statistics tracker
    pub fn new() -> Self {
        Self {
            symbol_usage: HashMap::new(),
            cached_max: None,
        }
    }

    /// Record a usage of a symbol
    ///
    /// This should be called whenever a symbol is referenced in the code:
    /// - Variable references
    /// - Method calls
    /// - Type usage
    /// - Imports
    ///
    /// # Arguments
    /// * `fqn` - Fully qualified name of the symbol
    ///
    /// # Example
    /// ```ignore
    /// stats.record_usage("java.util.ArrayList");
    /// stats.record_usage("com.example.UserService.getUser");
    /// ```
    pub fn record_usage(&mut self, fqn: &str) {
        let count = self.symbol_usage.entry(fqn.to_string()).or_insert(0);
        *count += 1;
        self.cached_max = None; // Invalidate cache

        debug!("Recorded usage: {} (count: {})", fqn, count);
    }

    /// Get the raw usage count for a symbol
    ///
    /// Returns 0 if the symbol has never been used.
    ///
    /// # Arguments
    /// * `fqn` - Fully qualified name of the symbol
    ///
    /// # Returns
    /// Number of times the symbol has been used
    pub fn get_usage(&self, fqn: &str) -> u32 {
        self.symbol_usage.get(fqn).copied().unwrap_or(0)
    }

    /// Get normalized usage score (0.0 to 1.0)
    ///
    /// Computes a score relative to the most-used symbol in the workspace.
    /// - Score of 1.0: Most frequently used symbol
    /// - Score of 0.5: Used half as much as the most frequent
    /// - Score of 0.0: Never used
    ///
    /// # Arguments
    /// * `fqn` - Fully qualified name of the symbol
    ///
    /// # Returns
    /// Normalized score between 0.0 and 1.0
    ///
    /// # Example
    /// ```ignore
    /// // ArrayList used 1000 times, HashMap used 500 times
    /// assert_eq!(stats.get_usage_score("java.util.ArrayList"), 1.0);
    /// assert_eq!(stats.get_usage_score("java.util.HashMap"), 0.5);
    /// ```
    pub fn get_usage_score(&self, fqn: &str) -> f64 {
        let count = self.get_usage(fqn);
        if count == 0 {
            return 0.0;
        }

        // Get or compute max usage count
        let max_usage = self.get_max_usage();
        if max_usage == 0 {
            return 0.0;
        }

        (count as f64) / (max_usage as f64)
    }

    /// Get the maximum usage count across all symbols
    fn get_max_usage(&self) -> u32 {
        if let Some(cached) = self.cached_max {
            return cached;
        }

        let max = self.symbol_usage.values().max().copied().unwrap_or(0);
        // Note: We don't cache here because &self is immutable
        max
    }

    /// Get total number of tracked symbols
    pub fn symbol_count(&self) -> usize {
        self.symbol_usage.len()
    }

    /// Get total usage count across all symbols
    pub fn total_usages(&self) -> u64 {
        self.symbol_usage.values().map(|&c| c as u64).sum()
    }

    /// Clear all usage statistics
    pub fn clear(&mut self) {
        self.symbol_usage.clear();
        self.cached_max = None;
    }

    /// Get the top N most used symbols
    ///
    /// Returns a vector of (FQN, count) pairs sorted by usage count (descending).
    ///
    /// # Arguments
    /// * `n` - Number of top symbols to return
    ///
    /// # Returns
    /// Vector of (FQN, usage_count) pairs, sorted by count descending
    pub fn get_top_symbols(&self, n: usize) -> Vec<(String, u32)> {
        let mut entries: Vec<_> = self
            .symbol_usage
            .iter()
            .map(|(fqn, &count)| (fqn.clone(), count))
            .collect();

        entries.sort_by_key(|&(_, count)| std::cmp::Reverse(count)); // Sort by count descending
        entries.truncate(n);
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_stats_empty() {
        let stats = UsageStatistics::new();
        assert_eq!(stats.symbol_count(), 0);
        assert_eq!(stats.total_usages(), 0);
    }

    #[test]
    fn test_record_usage() {
        let mut stats = UsageStatistics::new();

        stats.record_usage("java.util.ArrayList");
        assert_eq!(stats.get_usage("java.util.ArrayList"), 1);

        stats.record_usage("java.util.ArrayList");
        assert_eq!(stats.get_usage("java.util.ArrayList"), 2);

        stats.record_usage("java.util.HashMap");
        assert_eq!(stats.get_usage("java.util.HashMap"), 1);

        assert_eq!(stats.symbol_count(), 2);
        assert_eq!(stats.total_usages(), 3);
    }

    #[test]
    fn test_get_usage_nonexistent() {
        let stats = UsageStatistics::new();
        assert_eq!(stats.get_usage("java.util.ArrayList"), 0);
    }

    #[test]
    fn test_usage_score_normalization() {
        let mut stats = UsageStatistics::new();

        // Record different usage counts
        for _ in 0..100 {
            stats.record_usage("java.util.ArrayList");
        }
        for _ in 0..50 {
            stats.record_usage("java.util.HashMap");
        }
        for _ in 0..10 {
            stats.record_usage("java.util.LinkedList");
        }

        // Check normalized scores
        assert_eq!(stats.get_usage_score("java.util.ArrayList"), 1.0); // Most used
        assert_eq!(stats.get_usage_score("java.util.HashMap"), 0.5); // Half of max
        assert_eq!(stats.get_usage_score("java.util.LinkedList"), 0.1); // 10% of max
        assert_eq!(stats.get_usage_score("java.util.TreeMap"), 0.0); // Never used
    }

    #[test]
    fn test_top_symbols() {
        let mut stats = UsageStatistics::new();

        stats.record_usage("A");
        stats.record_usage("A");
        stats.record_usage("A");

        stats.record_usage("B");
        stats.record_usage("B");

        stats.record_usage("C");

        let top = stats.get_top_symbols(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0], ("A".to_string(), 3));
        assert_eq!(top[1], ("B".to_string(), 2));
    }

    #[test]
    fn test_clear() {
        let mut stats = UsageStatistics::new();
        stats.record_usage("java.util.ArrayList");
        stats.record_usage("java.util.HashMap");

        assert_eq!(stats.symbol_count(), 2);

        stats.clear();

        assert_eq!(stats.symbol_count(), 0);
        assert_eq!(stats.total_usages(), 0);
        assert_eq!(stats.get_usage("java.util.ArrayList"), 0);
    }
}
