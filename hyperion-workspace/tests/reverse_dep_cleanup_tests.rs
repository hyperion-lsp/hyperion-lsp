// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Reverse Dependency Memory Leak
// Tests that reverse dependency tracking is properly cleaned up when files are removed
// Current behavior: Removed files may still be tracked in reverse dependency map, causing memory leak
// Expected: Should remove all reverse dependency entries when a file is deleted

use std::collections::HashMap;

/// Mock structure representing reverse dependency tracking
struct ReverseDependencyTracker {
    /// Maps FQN -> files that import it
    reverse_deps: HashMap<String, Vec<usize>>,
    /// Maps file_id -> symbols it defines
    file_symbols: HashMap<usize, Vec<String>>,
}

impl ReverseDependencyTracker {
    fn new() -> Self {
        Self {
            reverse_deps: HashMap::new(),
            file_symbols: HashMap::new(),
        }
    }

    fn add_symbol(&mut self, file_id: usize, symbol: String, importers: Vec<usize>) {
        self.file_symbols
            .entry(file_id)
            .or_default()
            .push(symbol.clone());
        self.reverse_deps.insert(symbol, importers);
    }

    fn remove_file(&mut self, file_id: usize) {
        // Remove symbols defined by this file
        if let Some(symbols) = self.file_symbols.remove(&file_id) {
            for symbol in symbols {
                self.reverse_deps.remove(&symbol);
            }
        }

        // Remove this file from all importer lists
        for importers in self.reverse_deps.values_mut() {
            importers.retain(|&id| id != file_id);
        }

        // Clean up empty entries
        self.reverse_deps
            .retain(|_, importers| !importers.is_empty());
    }

    fn get_importers(&self, symbol: &str) -> Option<&Vec<usize>> {
        self.reverse_deps.get(symbol)
    }

    fn total_entries(&self) -> usize {
        self.reverse_deps.len()
    }

    fn total_file_symbols(&self) -> usize {
        self.file_symbols.len()
    }
}

#[test]
fn test_cleanup_on_file_removal() {
    let mut tracker = ReverseDependencyTracker::new();

    // File 1 defines com.example.User
    tracker.add_symbol(1, "com.example.User".to_string(), vec![2, 3]);

    // File 2 defines com.example.Product
    tracker.add_symbol(2, "com.example.Product".to_string(), vec![3]);

    assert_eq!(tracker.total_entries(), 2);
    assert_eq!(tracker.total_file_symbols(), 2);

    // Remove file 1
    tracker.remove_file(1);

    // com.example.User should be removed
    assert_eq!(tracker.get_importers("com.example.User"), None);
    assert_eq!(tracker.total_entries(), 1);
    assert_eq!(tracker.total_file_symbols(), 1);

    // com.example.Product should still exist
    assert_eq!(tracker.get_importers("com.example.Product"), Some(&vec![3]));
}

#[test]
fn test_cleanup_removes_file_from_importer_lists() {
    let mut tracker = ReverseDependencyTracker::new();

    // File 1 defines symbol, imported by files 2 and 3
    tracker.add_symbol(1, "com.example.User".to_string(), vec![2, 3]);

    // Remove file 2 (an importer)
    tracker.remove_file(2);

    // File 2 should be removed from importers list
    assert_eq!(tracker.get_importers("com.example.User"), Some(&vec![3]));
}

#[test]
fn test_cleanup_removes_empty_reverse_deps() {
    let mut tracker = ReverseDependencyTracker::new();

    // File 1 defines symbol, only imported by file 2
    tracker.add_symbol(1, "com.example.User".to_string(), vec![2]);

    assert_eq!(tracker.total_entries(), 1);

    // Remove file 2 (the only importer)
    tracker.remove_file(2);

    // Symbol should be removed entirely since no importers remain
    assert_eq!(tracker.get_importers("com.example.User"), None);
    assert_eq!(tracker.total_entries(), 0);
}

#[test]
fn test_cleanup_multiple_symbols_same_file() {
    let mut tracker = ReverseDependencyTracker::new();

    // File 1 defines multiple symbols
    tracker.add_symbol(1, "com.example.User".to_string(), vec![2]);
    tracker.add_symbol(1, "com.example.Product".to_string(), vec![2]);
    tracker.add_symbol(1, "com.example.Order".to_string(), vec![2]);

    assert_eq!(tracker.total_entries(), 3);
    assert_eq!(tracker.total_file_symbols(), 1);

    // Remove file 1
    tracker.remove_file(1);

    // All symbols should be removed
    assert_eq!(tracker.get_importers("com.example.User"), None);
    assert_eq!(tracker.get_importers("com.example.Product"), None);
    assert_eq!(tracker.get_importers("com.example.Order"), None);
    assert_eq!(tracker.total_entries(), 0);
    assert_eq!(tracker.total_file_symbols(), 0);
}

#[test]
fn test_cleanup_no_leak_after_churn() {
    let mut tracker = ReverseDependencyTracker::new();

    // Simulate file churn: add and remove many files
    for i in 0..100 {
        let symbol = format!("com.example.Class{}", i);
        tracker.add_symbol(i, symbol, vec![i + 1]);
    }

    assert_eq!(tracker.total_entries(), 100);
    assert_eq!(tracker.total_file_symbols(), 100);

    // Remove all files
    for i in 0..100 {
        tracker.remove_file(i);
    }

    // All entries should be cleaned up
    assert_eq!(tracker.total_entries(), 0);
    assert_eq!(tracker.total_file_symbols(), 0);
}

#[test]
fn test_cleanup_partial_removal() {
    let mut tracker = ReverseDependencyTracker::new();

    // Create interconnected dependencies
    tracker.add_symbol(1, "com.example.A".to_string(), vec![2, 3, 4]);
    tracker.add_symbol(2, "com.example.B".to_string(), vec![3, 4]);
    tracker.add_symbol(3, "com.example.C".to_string(), vec![4]);

    assert_eq!(tracker.total_entries(), 3);

    // Remove file 2
    tracker.remove_file(2);

    // com.example.B should be gone
    assert_eq!(tracker.get_importers("com.example.B"), None);

    // com.example.A should have file 2 removed from importers
    assert_eq!(tracker.get_importers("com.example.A"), Some(&vec![3, 4]));

    // com.example.C should still exist
    assert_eq!(tracker.get_importers("com.example.C"), Some(&vec![4]));

    assert_eq!(tracker.total_entries(), 2);
    assert_eq!(tracker.total_file_symbols(), 2);
}

#[test]
fn test_cleanup_double_removal() {
    let mut tracker = ReverseDependencyTracker::new();

    tracker.add_symbol(1, "com.example.User".to_string(), vec![2]);

    // Remove file 1 twice
    tracker.remove_file(1);
    tracker.remove_file(1);

    // Should handle gracefully
    assert_eq!(tracker.total_entries(), 0);
    assert_eq!(tracker.total_file_symbols(), 0);
}

#[test]
fn test_cleanup_nonexistent_file() {
    let mut tracker = ReverseDependencyTracker::new();

    tracker.add_symbol(1, "com.example.User".to_string(), vec![2]);

    // Remove file that doesn't exist
    tracker.remove_file(999);

    // Should not affect existing entries
    assert_eq!(tracker.get_importers("com.example.User"), Some(&vec![2]));
    assert_eq!(tracker.total_entries(), 1);
}

#[test]
fn test_cleanup_circular_dependencies() {
    let mut tracker = ReverseDependencyTracker::new();

    // File 1 -> File 2 -> File 3 -> File 1
    tracker.add_symbol(1, "com.example.A".to_string(), vec![2]);
    tracker.add_symbol(2, "com.example.B".to_string(), vec![3]);
    tracker.add_symbol(3, "com.example.C".to_string(), vec![1]);

    // Remove one file from the cycle
    tracker.remove_file(2);

    // com.example.B should be gone
    assert_eq!(tracker.get_importers("com.example.B"), None);

    // File 2 should be removed from com.example.A's importers
    // Since no importers remain, entry should be removed
    assert_eq!(tracker.get_importers("com.example.A"), None);

    // com.example.C should still exist
    assert_eq!(tracker.get_importers("com.example.C"), Some(&vec![1]));
}

#[test]
fn test_memory_stability_under_load() {
    let mut tracker = ReverseDependencyTracker::new();

    // Add many files
    for i in 0..1000 {
        let symbol = format!("com.example.Class{}", i);
        let importers: Vec<usize> = (0..10).map(|j| i * 10 + j).collect();
        tracker.add_symbol(i, symbol, importers);
    }

    let initial_entries = tracker.total_entries();
    assert_eq!(initial_entries, 1000);

    // Remove half the files
    for i in (0..1000).step_by(2) {
        tracker.remove_file(i);
    }

    // Should have approximately half the entries
    let remaining_entries = tracker.total_entries();
    assert_eq!(remaining_entries, 500);

    // Remove all remaining files
    for i in (1..1000).step_by(2) {
        tracker.remove_file(i);
    }

    // Should be completely clean
    assert_eq!(tracker.total_entries(), 0);
    assert_eq!(tracker.total_file_symbols(), 0);
}
