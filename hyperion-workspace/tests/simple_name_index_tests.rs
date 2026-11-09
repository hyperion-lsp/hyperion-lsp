// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

// Simple Name Index Stale Entries
// Tests that simple name index (short name -> FQN mapping) is properly updated on file changes
// Current behavior: Old simple name mappings may persist after class rename/move
// Expected: Should clean up old mappings and add new ones atomically

use std::collections::{HashMap, HashSet};

/// Mock structure representing simple name index
/// Maps simple name (e.g., "User") to set of FQNs (e.g., {"com.example.User", "com.other.User"})
struct SimpleNameIndex {
    /// Maps simple name -> set of FQNs
    name_to_fqns: HashMap<String, HashSet<String>>,
    /// Maps file_id -> set of FQNs it defines
    file_fqns: HashMap<usize, HashSet<String>>,
}

impl SimpleNameIndex {
    fn new() -> Self {
        Self {
            name_to_fqns: HashMap::new(),
            file_fqns: HashMap::new(),
        }
    }

    fn add_class(&mut self, file_id: usize, fqn: String) {
        let simple_name = Self::extract_simple_name(&fqn);

        self.name_to_fqns
            .entry(simple_name)
            .or_default()
            .insert(fqn.clone());

        self.file_fqns.entry(file_id).or_default().insert(fqn);
    }

    fn update_file(&mut self, file_id: usize, new_classes: Vec<String>) {
        // Remove old mappings for this file
        if let Some(old_fqns) = self.file_fqns.remove(&file_id) {
            for old_fqn in &old_fqns {
                let simple_name = Self::extract_simple_name(old_fqn);
                if let Some(fqns) = self.name_to_fqns.get_mut(&simple_name) {
                    fqns.remove(old_fqn);
                    if fqns.is_empty() {
                        self.name_to_fqns.remove(&simple_name);
                    }
                }
            }
        }

        // Add new mappings
        for fqn in new_classes {
            self.add_class(file_id, fqn);
        }
    }

    fn get_fqns(&self, simple_name: &str) -> Vec<String> {
        self.name_to_fqns
            .get(simple_name)
            .map(|set| {
                let mut vec: Vec<String> = set.iter().cloned().collect();
                vec.sort();
                vec
            })
            .unwrap_or_default()
    }

    fn extract_simple_name(fqn: &str) -> String {
        fqn.split('.').next_back().unwrap_or(fqn).to_string()
    }

    fn total_simple_names(&self) -> usize {
        self.name_to_fqns.len()
    }
}

#[test]
fn test_simple_name_basic_mapping() {
    let mut index = SimpleNameIndex::new();

    index.add_class(1, "com.example.User".to_string());

    assert_eq!(index.get_fqns("User"), vec!["com.example.User".to_string()]);
}

#[test]
fn test_simple_name_update_removes_old() {
    let mut index = SimpleNameIndex::new();

    // Initial: User class
    index.add_class(1, "com.example.User".to_string());

    assert_eq!(index.get_fqns("User"), vec!["com.example.User".to_string()]);

    // Update: rename to Customer
    index.update_file(1, vec!["com.example.Customer".to_string()]);

    // Old mapping should be gone
    assert_eq!(index.get_fqns("User"), Vec::<String>::new());
    assert_eq!(
        index.get_fqns("Customer"),
        vec!["com.example.Customer".to_string()]
    );
}

#[test]
fn test_simple_name_package_move() {
    let mut index = SimpleNameIndex::new();

    // Initial: com.example.User
    index.add_class(1, "com.example.User".to_string());

    assert_eq!(index.get_fqns("User"), vec!["com.example.User".to_string()]);

    // Update: move to com.example.model.User
    index.update_file(1, vec!["com.example.model.User".to_string()]);

    // Should update to new FQN
    let fqns = index.get_fqns("User");
    assert_eq!(fqns.len(), 1);
    assert!(fqns.contains(&"com.example.model.User".to_string()));
}

#[test]
fn test_simple_name_collision() {
    let mut index = SimpleNameIndex::new();

    // Two different files define classes with same simple name
    index.add_class(1, "com.example.User".to_string());
    index.add_class(2, "com.other.User".to_string());

    let fqns = index.get_fqns("User");
    assert_eq!(fqns.len(), 2);
    assert!(fqns.contains(&"com.example.User".to_string()));
    assert!(fqns.contains(&"com.other.User".to_string()));
}

#[test]
fn test_simple_name_collision_update() {
    let mut index = SimpleNameIndex::new();

    // Initial collision
    index.add_class(1, "com.example.User".to_string());
    index.add_class(2, "com.other.User".to_string());

    // Update file 1: rename to Customer
    index.update_file(1, vec!["com.example.Customer".to_string()]);

    // User should now only point to file 2's class
    assert_eq!(index.get_fqns("User"), vec!["com.other.User".to_string()]);
    assert_eq!(
        index.get_fqns("Customer"),
        vec!["com.example.Customer".to_string()]
    );
}

#[test]
fn test_simple_name_inner_class() {
    let mut index = SimpleNameIndex::new();

    index.add_class(1, "com.example.Outer.Inner".to_string());

    // Simple name should be "Inner"
    assert_eq!(
        index.get_fqns("Inner"),
        vec!["com.example.Outer.Inner".to_string()]
    );
}

#[test]
fn test_simple_name_multiple_classes_same_file() {
    let mut index = SimpleNameIndex::new();

    // File defines multiple top-level classes (unusual but valid)
    index.add_class(1, "com.example.User".to_string());
    index.add_class(1, "com.example.Product".to_string());

    assert_eq!(index.get_fqns("User"), vec!["com.example.User".to_string()]);
    assert_eq!(
        index.get_fqns("Product"),
        vec!["com.example.Product".to_string()]
    );

    // Update: now only defines User
    index.update_file(1, vec!["com.example.User".to_string()]);

    // Product should be gone
    assert_eq!(index.get_fqns("Product"), Vec::<String>::new());
    assert_eq!(index.get_fqns("User"), vec!["com.example.User".to_string()]);
}

#[test]
fn test_simple_name_nested_inner_classes() {
    let mut index = SimpleNameIndex::new();

    index.add_class(1, "com.example.Outer".to_string());
    index.add_class(1, "com.example.Outer.Inner".to_string());
    index.add_class(1, "com.example.Outer.Inner.DeepNested".to_string());

    assert_eq!(
        index.get_fqns("Outer"),
        vec!["com.example.Outer".to_string()]
    );
    assert_eq!(
        index.get_fqns("Inner"),
        vec!["com.example.Outer.Inner".to_string()]
    );
    assert_eq!(
        index.get_fqns("DeepNested"),
        vec!["com.example.Outer.Inner.DeepNested".to_string()]
    );

    // Update: remove all
    index.update_file(1, vec![]);

    assert_eq!(index.get_fqns("Outer"), Vec::<String>::new());
    assert_eq!(index.get_fqns("Inner"), Vec::<String>::new());
    assert_eq!(index.get_fqns("DeepNested"), Vec::<String>::new());
}

#[test]
fn test_simple_name_cleanup_on_empty_update() {
    let mut index = SimpleNameIndex::new();

    index.add_class(1, "com.example.User".to_string());
    assert_eq!(index.total_simple_names(), 1);

    // Update with empty list (file deleted or became empty)
    index.update_file(1, vec![]);

    assert_eq!(index.total_simple_names(), 0);
    assert_eq!(index.get_fqns("User"), Vec::<String>::new());
}

#[test]
fn test_simple_name_generic_class() {
    let mut index = SimpleNameIndex::new();

    // Generic classes: simple name doesn't include type parameters
    index.add_class(1, "com.example.Container".to_string());

    assert_eq!(
        index.get_fqns("Container"),
        vec!["com.example.Container".to_string()]
    );
}

#[test]
fn test_simple_name_same_name_different_packages() {
    let mut index = SimpleNameIndex::new();

    // Common pattern: java.util.List vs java.awt.List
    index.add_class(1, "java.util.List".to_string());
    index.add_class(2, "java.awt.List".to_string());

    let fqns = index.get_fqns("List");
    assert_eq!(fqns.len(), 2);
    assert!(fqns.contains(&"java.util.List".to_string()));
    assert!(fqns.contains(&"java.awt.List".to_string()));
}

#[test]
fn test_simple_name_rapid_updates() {
    let mut index = SimpleNameIndex::new();

    // Simulate rapid file changes
    for i in 0..100 {
        index.update_file(1, vec![format!("com.example.Class{}", i)]);

        // Should always have exactly 1 simple name for this file
        let name = format!("Class{}", i);
        assert_eq!(index.get_fqns(&name).len(), 1);

        // Old names should be cleaned up
        if i > 0 {
            let old_name = format!("Class{}", i - 1);
            assert_eq!(index.get_fqns(&old_name), Vec::<String>::new());
        }
    }
}

#[test]
fn test_simple_name_no_package() {
    let mut index = SimpleNameIndex::new();

    // Default package (no dots in FQN)
    index.add_class(1, "User".to_string());

    assert_eq!(index.get_fqns("User"), vec!["User".to_string()]);
}

#[test]
fn test_simple_name_collision_with_inner_class() {
    let mut index = SimpleNameIndex::new();

    // Same simple name for outer and inner class in different files
    index.add_class(1, "com.example.Node".to_string());
    index.add_class(2, "com.example.Tree.Node".to_string());

    let fqns = index.get_fqns("Node");
    assert_eq!(fqns.len(), 2);
    assert!(fqns.contains(&"com.example.Node".to_string()));
    assert!(fqns.contains(&"com.example.Tree.Node".to_string()));
}

#[test]
fn test_simple_name_single_letter() {
    let mut index = SimpleNameIndex::new();

    // Single letter class names
    index.add_class(1, "com.example.A".to_string());
    index.add_class(2, "com.example.B".to_string());

    assert_eq!(index.get_fqns("A"), vec!["com.example.A".to_string()]);
    assert_eq!(index.get_fqns("B"), vec!["com.example.B".to_string()]);
}

#[test]
fn test_simple_name_high_collision_count() {
    let mut index = SimpleNameIndex::new();

    // Many files define classes with same simple name
    for i in 0..50 {
        index.add_class(i, format!("com.example.package{}.User", i));
    }

    let fqns = index.get_fqns("User");
    assert_eq!(fqns.len(), 50);
}

#[test]
fn test_simple_name_special_characters() {
    let mut index = SimpleNameIndex::new();

    // Class names with special characters (valid in Java)
    index.add_class(1, "com.example.User$Builder".to_string());
    index.add_class(2, "com.example._Internal".to_string());

    assert_eq!(
        index.get_fqns("User$Builder"),
        vec!["com.example.User$Builder".to_string()]
    );
    assert_eq!(
        index.get_fqns("_Internal"),
        vec!["com.example._Internal".to_string()]
    );
}

#[test]
fn test_simple_name_update_idempotency() {
    let mut index = SimpleNameIndex::new();

    let classes = vec!["com.example.User".to_string()];

    // Update multiple times with same data
    index.update_file(1, classes.clone());
    index.update_file(1, classes.clone());
    index.update_file(1, classes.clone());

    // Should still have single mapping
    assert_eq!(index.get_fqns("User"), vec!["com.example.User".to_string()]);
    assert_eq!(index.total_simple_names(), 1);
}
