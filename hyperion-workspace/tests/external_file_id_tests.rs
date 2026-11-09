// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// External File ID Overflow
// Tests that file ID assignment handles large numbers of files without overflow
// Current behavior: File IDs may overflow or wrap around with very large codebases
// Expected: Should use appropriate integer type and handle 1M+ files

use std::collections::HashMap;

/// Mock structure representing file ID management
struct FileIdManager {
    next_id: usize,
    path_to_id: HashMap<String, usize>,
    id_to_path: HashMap<usize, String>,
}

impl FileIdManager {
    fn new() -> Self {
        Self {
            next_id: 0,
            path_to_id: HashMap::new(),
            id_to_path: HashMap::new(),
        }
    }

    fn register_file(&mut self, path: String) -> usize {
        if let Some(&id) = self.path_to_id.get(&path) {
            return id;
        }

        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).expect("File ID overflow");

        self.path_to_id.insert(path.clone(), id);
        self.id_to_path.insert(id, path);

        id
    }

    fn get_id(&self, path: &str) -> Option<usize> {
        self.path_to_id.get(path).copied()
    }

    fn get_path(&self, id: usize) -> Option<&String> {
        self.id_to_path.get(&id)
    }

    fn total_files(&self) -> usize {
        self.path_to_id.len()
    }

    fn remove_file(&mut self, path: &str) -> bool {
        if let Some(id) = self.path_to_id.remove(path) {
            self.id_to_path.remove(&id);
            true
        } else {
            false
        }
    }
}

#[test]
fn test_file_id_sequential_assignment() {
    let mut manager = FileIdManager::new();

    let id1 = manager.register_file("File1.java".to_string());
    let id2 = manager.register_file("File2.java".to_string());
    let id3 = manager.register_file("File3.java".to_string());

    assert_eq!(id1, 0);
    assert_eq!(id2, 1);
    assert_eq!(id3, 2);
}

#[test]
fn test_file_id_deduplication() {
    let mut manager = FileIdManager::new();

    let id1 = manager.register_file("File.java".to_string());
    let id2 = manager.register_file("File.java".to_string());
    let id3 = manager.register_file("File.java".to_string());

    assert_eq!(id1, id2);
    assert_eq!(id2, id3);
    assert_eq!(manager.total_files(), 1);
}

#[test]
fn test_file_id_lookup() {
    let mut manager = FileIdManager::new();

    manager.register_file("com/example/User.java".to_string());
    manager.register_file("com/example/Product.java".to_string());

    assert_eq!(manager.get_id("com/example/User.java"), Some(0));
    assert_eq!(manager.get_id("com/example/Product.java"), Some(1));
    assert_eq!(manager.get_id("com/example/NotFound.java"), None);
}

#[test]
fn test_file_id_reverse_lookup() {
    let mut manager = FileIdManager::new();

    let id1 = manager.register_file("File1.java".to_string());
    let id2 = manager.register_file("File2.java".to_string());

    assert_eq!(manager.get_path(id1), Some(&"File1.java".to_string()));
    assert_eq!(manager.get_path(id2), Some(&"File2.java".to_string()));
    assert_eq!(manager.get_path(999), None);
}

#[test]
fn test_file_id_large_scale() {
    let mut manager = FileIdManager::new();

    // Register 10,000 files
    for i in 0..10_000 {
        let path = format!("com/example/Class{}.java", i);
        let id = manager.register_file(path.clone());
        assert_eq!(id, i);
    }

    assert_eq!(manager.total_files(), 10_000);

    // Verify all lookups work
    for i in 0..10_000 {
        let path = format!("com/example/Class{}.java", i);
        assert_eq!(manager.get_id(&path), Some(i));
    }
}

#[test]
fn test_file_id_after_removal() {
    let mut manager = FileIdManager::new();

    manager.register_file("File1.java".to_string());
    manager.register_file("File2.java".to_string());

    // Remove File1
    assert!(manager.remove_file("File1.java"));

    // Next registration should continue sequence (not reuse ID 0)
    let id = manager.register_file("File3.java".to_string());
    assert_eq!(id, 2);

    assert_eq!(manager.total_files(), 2);
}

#[test]
fn test_file_id_with_special_characters() {
    let mut manager = FileIdManager::new();

    let paths = [
        "File with spaces.java",
        "File-with-dashes.java",
        "File_with_underscores.java",
        "File.with.dots.java",
        "File$with$dollars.java",
    ];

    for (i, path) in paths.iter().enumerate() {
        let id = manager.register_file(path.to_string());
        assert_eq!(id, i);
    }

    assert_eq!(manager.total_files(), 5);
}

#[test]
fn test_file_id_nested_packages() {
    let mut manager = FileIdManager::new();

    let id1 = manager.register_file("com/example/User.java".to_string());
    let id2 = manager.register_file("com/example/model/User.java".to_string());
    let id3 = manager.register_file("com/example/model/internal/User.java".to_string());

    // All should get unique IDs even with same filename
    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);

    assert_eq!(manager.total_files(), 3);
}

#[test]
fn test_file_id_boundary_values() {
    let mut manager = FileIdManager::new();

    // Test that we can handle IDs near boundaries
    // Pre-register many files to push ID counter high
    for i in 0..1000 {
        manager.register_file(format!("File{}.java", i));
    }

    assert_eq!(manager.total_files(), 1000);

    // Continue registering
    let id = manager.register_file("NewFile.java".to_string());
    assert_eq!(id, 1000);
}

#[test]
#[should_panic(expected = "File ID overflow")]
fn test_file_id_overflow_detection() {
    let mut manager = FileIdManager::new();

    // Set next_id to near max
    manager.next_id = usize::MAX;

    // This should panic with overflow detection
    manager.register_file("File.java".to_string());
}

#[test]
fn test_file_id_concurrent_safe_values() {
    let mut manager = FileIdManager::new();

    // Verify IDs are within safe range for concurrent operations
    for i in 0..1000 {
        let id = manager.register_file(format!("File{}.java", i));
        assert!(id < usize::MAX / 2, "ID should be in safe range");
    }
}

#[test]
fn test_file_id_persistence_across_operations() {
    let mut manager = FileIdManager::new();

    let path = "com/example/User.java".to_string();

    let id1 = manager.register_file(path.clone());
    manager.remove_file(&path);
    let id2 = manager.register_file(path.clone());

    // New registration after removal gets new ID
    assert_ne!(id1, id2);
}

#[test]
fn test_file_id_empty_path() {
    let mut manager = FileIdManager::new();

    // Empty path should still get an ID (even though invalid)
    let id = manager.register_file("".to_string());
    assert_eq!(id, 0);
    assert_eq!(manager.get_path(id), Some(&"".to_string()));
}

#[test]
fn test_file_id_remove_nonexistent() {
    let mut manager = FileIdManager::new();

    manager.register_file("File1.java".to_string());

    // Removing non-existent file should return false
    assert!(!manager.remove_file("File2.java"));

    assert_eq!(manager.total_files(), 1);
}

#[test]
fn test_file_id_churn() {
    let mut manager = FileIdManager::new();

    // Simulate high file churn
    for _ in 0..100 {
        for i in 0..10 {
            let path = format!("File{}.java", i);
            manager.register_file(path.clone());
            manager.remove_file(&path);
        }
    }

    // IDs should have advanced significantly
    let id = manager.register_file("Final.java".to_string());
    assert!(id >= 1000, "ID should reflect all registrations");
}

#[test]
fn test_file_id_stress_large_paths() {
    let mut manager = FileIdManager::new();

    // Very long package paths
    for i in 0..1000 {
        let path = format!(
            "com/example/very/deep/package/structure/level1/level2/level3/level4/Class{}.java",
            i
        );
        manager.register_file(path);
    }

    assert_eq!(manager.total_files(), 1000);
}
